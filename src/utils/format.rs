//! 统一的人类可读单位格式化工具。
//!
//! 此前 dashboard / adapter / traffic 各自维护了一份精度不一致的副本
//! （MB/s 有的 `.1` 有的 `.2`，GB 有的 `.1` 有的 `.2`）。这里收敛为单一实现，
//! 避免显示风格漂移，并提供单元测试锁定行为。

/// 将"字节/秒"格式化为速率字符串（二进制单位，1 KB = 1024 B）。
pub fn format_speed(bytes_per_sec: u64) -> String {
    let kbps = bytes_per_sec as f64 / 1024.0;
    if kbps < 1024.0 {
        format!("{:.1} KB/s", kbps)
    } else {
        format!("{:.2} MB/s", kbps / 1024.0)
    }
}

/// 将字节数格式化为可读体积字符串（二进制单位）。
pub fn format_bytes(bytes: u64) -> String {
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        format!("{:.1} KB", kb)
    } else if kb < 1024.0 * 1024.0 {
        format!("{:.1} MB", kb / 1024.0)
    } else {
        format!("{:.2} GB", kb / 1024.0 / 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_below_one_mb_uses_kb() {
        assert_eq!(format_speed(0), "0.0 KB/s");
        assert_eq!(format_speed(512), "0.5 KB/s");
        assert_eq!(format_speed(1023 * 1024), "1023.0 KB/s");
    }

    #[test]
    fn speed_at_or_above_one_mb_uses_mb() {
        assert_eq!(format_speed(1024 * 1024), "1.00 MB/s");
        assert_eq!(format_speed(2 * 1024 * 1024), "2.00 MB/s");
    }

    #[test]
    fn bytes_scale_across_units() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
    }
}
