//! 「最近使用」历史（MRU）。供各目标/CIDR 输入框做灰字补全与下拉回选。
//!
//! 两个独立池：`targets`（IP/主机，跨 ping/trace/端口扫描/链路质量/内网对端共享）
//! 与 `cidrs`（扫描网段，独立）。纯逻辑、可单测；UI 交互在 `ui/mru.rs`。

/// 单个 MRU 池：去重 + 最近在前 + 截断到 `cap`。
#[derive(Debug, Clone, PartialEq)]
pub struct History {
    items: Vec<String>,
    cap: usize,
}

impl History {
    pub fn new(cap: usize) -> Self {
        Self {
            items: Vec::new(),
            cap: cap.max(1),
        }
    }

    /// 从持久化的 Vec 构造（截断到 cap，保持顺序=最近在前）。
    pub fn from_vec(items: Vec<String>, cap: usize) -> Self {
        let mut h = Self::new(cap);
        // 持久化格式已按「最近在前」存储，直接按既有顺序填充并截断即可保持顺序。
        h.items = items.into_iter().filter(|s| !s.trim().is_empty()).collect();
        h.items.truncate(h.cap);
        h
    }

    pub fn to_vec(&self) -> Vec<String> {
        self.items.clone()
    }

    pub fn entries(&self) -> &[String] {
        &self.items
    }

    /// 记录一次「使用」：trim 后空则忽略；已存在则移到最前；插到最前；截断到 cap。
    pub fn record(&mut self, v: &str) {
        let v = v.trim();
        if v.is_empty() {
            return;
        }
        self.items.retain(|x| x != v);
        self.items.insert(0, v.to_string());
        self.items.truncate(self.cap);
    }

    /// 前缀建议：返回以 `input` 为前缀且 ≠ `input` 的最近一条（最近在前→首个命中）。
    /// `input` trim 后为空返回 None（空框不打扰）。
    /// 不变量：存储条目均经 `record` 的 trim 处理，故 `q`(trim 后) 与条目的 `starts_with`/`!=` 比较自洽。
    pub fn suggest(&self, input: &str) -> Option<String> {
        let q = input.trim();
        if q.is_empty() {
            return None;
        }
        self.items
            .iter()
            .find(|x| x.starts_with(q) && x.as_str() != q)
            .cloned()
    }
}

/// 三池聚合，由 App 持有（`Rc<RefCell<HistoryStore>>`）并 clone 进各工具。
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryStore {
    pub targets: History,
    pub cidrs: History,
    /// 适配器编辑专用（IP/掩码/网关/DNS 共享）
    pub adapter: History,
}

const HISTORY_CAP: usize = 15;

impl Default for HistoryStore {
    fn default() -> Self {
        Self {
            targets: History::new(HISTORY_CAP),
            cidrs: History::new(HISTORY_CAP),
            adapter: History::new(HISTORY_CAP),
        }
    }
}

impl HistoryStore {
    pub fn from_persist(p: &crate::session::HistoryPersist) -> Self {
        Self {
            targets: History::from_vec(p.targets.clone(), HISTORY_CAP),
            cidrs: History::from_vec(p.cidrs.clone(), HISTORY_CAP),
            adapter: History::from_vec(p.adapter.clone(), HISTORY_CAP),
        }
    }

    pub fn to_persist(&self) -> crate::session::HistoryPersist {
        crate::session::HistoryPersist {
            targets: self.targets.to_vec(),
            cidrs: self.cidrs.to_vec(),
            adapter: self.adapter.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_dedup_moves_to_front_and_caps() {
        let mut h = History::new(3);
        h.record("a");
        h.record("b");
        h.record("a"); // 去重 + 移到最前
        assert_eq!(h.entries(), &["a", "b"]);
        h.record("c");
        h.record("d"); // 超过 cap=3，最旧的 "b" 被挤出
        assert_eq!(h.entries(), &["d", "c", "a"]);
    }

    #[test]
    fn record_ignores_blank() {
        let mut h = History::new(5);
        h.record("   ");
        h.record("");
        assert!(h.entries().is_empty());
    }

    #[test]
    fn suggest_prefix_excludes_self_and_blank() {
        let mut h = History::new(5);
        h.record("192.168.1.50");
        h.record("192.168.1.1");
        // 最近在前：先命中 "192.168.1.1"
        assert_eq!(h.suggest("192"), Some("192.168.1.1".to_string()));
        // 完整等于某条 → 排除自身，命中更早的同前缀项
        assert_eq!(h.suggest("192.168.1.1"), None);
        // 空输入不建议
        assert_eq!(h.suggest("   "), None);
        // 无前缀命中
        assert_eq!(h.suggest("10."), None);
    }

    #[test]
    fn from_vec_filters_blank_and_truncates() {
        let items = vec![
            "a".to_string(),
            "  ".to_string(),
            "b".to_string(),
            "c".to_string(),
        ];
        let h = History::from_vec(items, 2);
        assert_eq!(h.entries(), &["a", "b"]); // 空白被滤掉，截断到 cap=2
    }

    #[test]
    fn store_persist_roundtrip() {
        let mut s = HistoryStore::default();
        s.targets.record("8.8.8.8");
        s.cidrs.record("192.168.1.0/24");
        s.adapter.record("192.168.1.1");
        let p = s.to_persist();
        let back = HistoryStore::from_persist(&p);
        assert_eq!(s, back);
    }
}
