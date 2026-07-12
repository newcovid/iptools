use crate::{
    LinkQualityDimension, LinkQualityDimensionKind, LinkQualityGrade, LinkQualitySample,
    LinkQualitySnapshot, LinkQualitySummary,
};

pub fn summary_from_sample(
    snapshot: &LinkQualitySnapshot,
    sample: &LinkQualitySample,
) -> LinkQualitySummary {
    let dimensions = dimensions(
        snapshot.adapter.is_wifi,
        sample.average_latency_ms,
        sample.jitter_ms,
        sample.loss_percent,
        sample.average_rssi_dbm,
        snapshot
            .wireless
            .as_ref()
            .map(|wireless| wireless.tx_rate_mbps),
        snapshot
            .wireless
            .as_ref()
            .map(|wireless| wireless.wifi_generation),
    );
    let score = overall(
        &dimensions
            .iter()
            .map(|dimension| (dimension.score, dimension.weight))
            .collect::<Vec<_>>(),
    );
    let grade = grade_from_score(score);
    let weakest = dimensions
        .iter()
        .min_by(|left, right| left.score.total_cmp(&right.score))
        .map(|dimension| dimension.kind);
    LinkQualitySummary {
        score,
        grade,
        weakest,
        dimensions,
        sent: sample.sent,
        received: sample.received,
        min_latency_ms: sample.min_latency_ms,
        average_latency_ms: sample.average_latency_ms,
        max_latency_ms: sample.max_latency_ms,
        jitter_ms: sample.jitter_ms,
        loss_percent: sample.loss_percent,
        min_rssi_dbm: sample.min_rssi_dbm,
        average_rssi_dbm: sample.average_rssi_dbm,
        max_rssi_dbm: sample.max_rssi_dbm,
        min_signal_quality: sample.min_signal_quality,
        average_signal_quality: sample.average_signal_quality,
        max_signal_quality: sample.max_signal_quality,
        link_speed_bps: sample.link_speed_bps.or(snapshot.adapter.link_speed_bps),
    }
}

pub fn dimensions(
    is_wifi: bool,
    average_latency_ms: Option<f64>,
    jitter_ms: Option<f64>,
    loss_percent: f64,
    average_rssi_dbm: Option<f64>,
    tx_rate_mbps: Option<u32>,
    wifi_generation: Option<u8>,
) -> Vec<LinkQualityDimension> {
    let latency = average_latency_ms.map_or(0.0, latency_score);
    let jitter = jitter_ms.map_or(0.0, jitter_score);
    let loss = loss_score(loss_percent);
    if is_wifi {
        vec![
            dimension(LinkQualityDimensionKind::Loss, loss, 25.0),
            dimension(LinkQualityDimensionKind::Latency, latency, 20.0),
            dimension(LinkQualityDimensionKind::Jitter, jitter, 15.0),
            dimension(
                LinkQualityDimensionKind::Signal,
                signal_score(average_rssi_dbm.unwrap_or(-100.0)),
                25.0,
            ),
            dimension(
                LinkQualityDimensionKind::Rate,
                rate_score(tx_rate_mbps.unwrap_or_default() as f64),
                10.0,
            ),
            dimension(
                LinkQualityDimensionKind::Phy,
                phy_score(wifi_generation.unwrap_or_default()),
                5.0,
            ),
        ]
    } else {
        vec![
            dimension(LinkQualityDimensionKind::Loss, loss, 40.0),
            dimension(LinkQualityDimensionKind::Latency, latency, 35.0),
            dimension(LinkQualityDimensionKind::Jitter, jitter, 25.0),
        ]
    }
}

fn dimension(kind: LinkQualityDimensionKind, score: f64, weight: f64) -> LinkQualityDimension {
    LinkQualityDimension {
        kind,
        score,
        weight,
    }
}

pub fn lerp_score(value: f64, best: f64, worst: f64) -> f64 {
    if (best - worst).abs() < f64::EPSILON {
        return 0.0;
    }
    ((value - worst) / (best - worst)).clamp(0.0, 1.0) * 100.0
}

pub fn latency_score(value: f64) -> f64 {
    lerp_score(value, 20.0, 300.0)
}

pub fn jitter_score(value: f64) -> f64 {
    lerp_score(value, 2.0, 80.0)
}

pub fn loss_score(value: f64) -> f64 {
    lerp_score(value, 0.0, 10.0)
}

pub fn signal_score(value: f64) -> f64 {
    lerp_score(value, -50.0, -85.0)
}

pub fn rate_score(value: f64) -> f64 {
    lerp_score(value, 433.0, 6.0)
}

pub fn phy_score(generation: u8) -> f64 {
    match generation {
        7 | 6 => 100.0,
        5 => 80.0,
        4 => 60.0,
        _ => 30.0,
    }
}

pub fn overall(dimensions: &[(f64, f64)]) -> f64 {
    let weight: f64 = dimensions.iter().map(|(_, weight)| weight).sum();
    if weight <= 0.0 {
        return 0.0;
    }
    dimensions
        .iter()
        .map(|(score, weight)| score * weight)
        .sum::<f64>()
        / weight
}

pub fn grade_from_score(score: f64) -> LinkQualityGrade {
    if score >= 85.0 {
        LinkQualityGrade::Excellent
    } else if score >= 70.0 {
        LinkQualityGrade::Good
    } else if score >= 50.0 {
        LinkQualityGrade::Fair
    } else {
        LinkQualityGrade::Poor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v031_scoring_boundaries_are_preserved() {
        assert_eq!(latency_score(20.0), 100.0);
        assert_eq!(latency_score(300.0), 0.0);
        assert!((latency_score(160.0) - 50.0).abs() < 1.0);
        assert_eq!(loss_score(0.0), 100.0);
        assert_eq!(loss_score(10.0), 0.0);
        assert_eq!(signal_score(-50.0), 100.0);
        assert_eq!(signal_score(-85.0), 0.0);
        assert_eq!(rate_score(433.0), 100.0);
        assert_eq!(rate_score(6.0), 0.0);
        assert_eq!(phy_score(6), 100.0);
        assert_eq!(phy_score(5), 80.0);
        assert_eq!(phy_score(4), 60.0);
    }

    #[test]
    fn v031_weights_and_grades_are_preserved() {
        assert_eq!(
            overall(&[(100.0, 40.0), (100.0, 35.0), (100.0, 25.0)]),
            100.0
        );
        assert_eq!(grade_from_score(86.0), LinkQualityGrade::Excellent);
        assert_eq!(grade_from_score(72.0), LinkQualityGrade::Good);
        assert_eq!(grade_from_score(55.0), LinkQualityGrade::Fair);
        assert_eq!(grade_from_score(40.0), LinkQualityGrade::Poor);
    }
}
