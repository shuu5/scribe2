//! 統合 test の唯一の target（憲法 R-C13-2「統合 test file 3 以下」は cargo の
//! integration test **target** の数で数える。以後の leg は module で足す）。

mod fleet;
mod hook;
mod rules;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 同一 process 内での dir 名衝突を避ける連番。
static SEQ: AtomicU32 = AtomicU32::new(0);

/// repo の外に一意な tmp dir を作る。
///
/// `tempfile` は直接依存の追加（憲法 A3）に当たるので足さない。xtask の
/// `make_tmp_dir` と同形の std だけの helper である。
pub fn make_tmp_dir() -> Option<PathBuf> {
    let base = std::env::temp_dir();
    for _ in 0..8 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("e2e-{}-{nanos}-{seq}", std::process::id()));
        if std::fs::create_dir(&dir).is_ok() {
            return Some(dir);
        }
    }
    None
}
