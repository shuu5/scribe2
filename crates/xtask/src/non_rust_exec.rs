//! `non-rust-exec`（tracked な**非 Rust 実行物**は manifest の例外行に載るものだけ）と
//! `ci-shell-lines`（CI の shell 行の**検出線**）。ADR-0009 §2.5。
//!
//! 憲法 C12 は「歯は Rust 1 framework」と定める。放っておくと、その線は**歯以外の実行物**
//! （生成 script・hook・asset）から静かに崩れる——bash で 1 本書いた瞬間に「2 つ目の
//! framework」が repo に住み着く。ゆえに **file 種別だけで閉じた分類器**で数え、例外は
//! **manifest の 1 面**（`repo.non_rust_exec_allow`・裁定 id つき）に置く。
//!
//! **prose の字面 grep で書かない**のが要点である。「script っぽい名前」で分けると、
//! 名前を変えるだけで抜けられる。見るのは (a) 先頭 2 byte の `#!` (b) index の実行 bit
//! (c) 拡張子の閉じた列、の 3 つだけで、どれも file の**種別**である。

use crate::check::{failed, Layout, Measured};
use crate::paths_clean::{tracked_files, Tracked, TrackedFile};
use crate::toml_lite;
use std::path::Path;

/// 非 Rust 実行物と見なす拡張子（**閉じた列**）。
///
/// `js` を外さない——asset（minify 済みの library 等）は**例外行に載せて**通す。定義を
/// 緩めると次の asset が黙って通り、線が「無いのと同じ」になる（契約の「やらない」）。
const EXEC_EXTENSIONS: &[&str] = &["sh", "bash", "zsh", "py", "bats", "pl", "rb", "js", "ts", "mjs"];

/// index が実行 bit を付ける mode。
const EXEC_MODE: &str = "100755";

/// 例外 path を持つ rules 行の id（**値は manifest が持つ**・憲法 C1）。
const ALLOW_ROW: &str = "repo.non_rust_exec_allow";

/// array-of-tables の section header の字面（`sections` は `[` を 1 つだけ剥がす）。
const RULE_HEADER: &str = "[rule";

/// UTF-8 の BOM（editor が黙って足す 3 byte）。
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// file の種別が「非 Rust 実行物」か。**種別だけ**を見る（本文の字面は見ない）。
///
/// 拡張子は **大文字小文字を区別しない**（`.PY` は「拡張子 py の file」である）。shebang は
/// **BOM の後ろでも見る**（BOM 付きの shebang も shebang である）。どちらも線引きの話ではなく、
/// **同じ signal を取り落とさない**ための正規化である（lens 2026-09-11）。
pub(crate) fn is_non_rust_exec(file: &TrackedFile, body: Option<&[u8]>) -> bool {
    if file.mode == EXEC_MODE {
        return true;
    }
    if body.is_some_and(has_shebang) {
        return true;
    }
    extension_of(&file.rel)
        .is_some_and(|ext| EXEC_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// 先頭が shebang か（BOM を 1 つだけ剥がしてから見る）。
fn has_shebang(bytes: &[u8]) -> bool {
    let head = bytes.strip_prefix(BOM).unwrap_or(bytes);
    head.starts_with(b"#!")
}

/// path の拡張子（`.` の後ろ・小文字化しない＝index の字面のまま見る）。
fn extension_of(rel: &str) -> Option<&str> {
    let name = rel.rsplit('/').next()?;
    let (_, ext) = name.rsplit_once('.')?;
    if ext.is_empty() {
        return None;
    }
    Some(ext)
}

/// manifest の例外行（**完全一致**の path の列）。行が無ければ空。
pub(crate) fn allow_list(manifest: &str) -> Vec<String> {
    for (header, pairs) in toml_lite::sections(manifest) {
        if header != RULE_HEADER {
            continue;
        }
        let id = pairs
            .iter()
            .find(|(key, _)| *key == "id")
            .and_then(|(_, value)| toml_lite::quoted(value));
        if id.as_deref() != Some(ALLOW_ROW) {
            continue;
        }
        return pairs
            .iter()
            .find(|(key, _)| *key == "value")
            .map(|(_, value)| quoted_items(value))
            .unwrap_or_default();
    }
    Vec::new()
}

/// `["a", "b"]` の各要素を取り出す（json/toml の array の subset）。
///
/// **`[` から `]` までの中だけ**を読む。行の残り（末尾コメント）まで走査すると、
/// `value = [...] # 旧 "tools/pwned.sh" は外した` の**注記が例外を 1 件増やす**——
/// 例外を減らす意図の文が静かに例外を足す形で、憲法 C1 の「値の面は 1 つ」が崩れる
/// （lens 2026-09-11 H2・実測で `allow=3` になった）。
fn quoted_items(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let Some(open) = value.find('[') else {
        return items;
    };
    let after_open = value.get(open.saturating_add(1)..).unwrap_or_default();
    let span = match after_open.find(']') {
        Some(close) => after_open.get(..close).unwrap_or_default(),
        // 閉じ括弧が無い行は**読めていない**＝要素を 1 つも採らない（部分的に採ると
        // 「途中まで読めた分」が例外として効いてしまう）。
        None => return items,
    };
    let mut rest = span;
    while let Some(at) = rest.find('"') {
        let after = rest.get(at.saturating_add(1)..).unwrap_or_default();
        let Some((item, tail)) = after.split_once('"') else {
            break;
        };
        items.push(item.to_owned());
        rest = tail;
    }
    items
}

/// tracked な非 Rust 実行物は例外行に載るものだけ（`non-rust-exec`）。
pub(crate) fn measure(layout: &Layout) -> Measured {
    let manifest = match std::fs::read_to_string(layout.root.join("rules").join("manifest.toml")) {
        Ok(text) => text,
        Err(err) => return failed("non-rust-exec", &format!("manifest を読めない: {err}")),
    };
    match tracked_files(&layout.root) {
        Tracked::Unmeasurable(reason) => failed("non-rust-exec", &reason),
        // flip-check の base tree は repo root ではない（`paths-clean` と同じ枝）。
        Tracked::NotRepoRoot => Measured {
            fact: "non-rust-exec=n/a(not-a-repo-root)".to_owned(),
            violations: Vec::new(),
        },
        // **母集団 0 は「測れなかった」**（0 件を緑にしない・`paths-clean` と同じ極性）。
        Tracked::Listed(listed) if listed.is_empty() => {
            failed("non-rust-exec", "tracked file が 0 件である")
        }
        Tracked::Listed(listed) => judge(&layout.root, &listed, &allow_list(&manifest)),
    }
}

/// 母集団を 1 本ずつ分類し、例外に載らない件を違反にする。
fn judge(root: &Path, listed: &[TrackedFile], allow: &[String]) -> Measured {
    let mut violations = Vec::new();
    let mut hits = 0_usize;
    for file in listed {
        let body = std::fs::read(root.join(&file.rel)).ok();
        if !is_non_rust_exec(file, body.as_deref()) {
            continue;
        }
        hits = hits.saturating_add(1);
        if allow.iter().any(|listed_path| listed_path == &file.rel) {
            continue;
        }
        violations.push(format!(
            "non-rust-exec: {} は非 Rust 実行物である（例外は manifest の {ALLOW_ROW} 行だけ）",
            file.rel
        ));
    }
    // **腐った例外行を落とす**（憲法 C10.3: 未配線の設定は CI が落とす）。母集団に居ない
    // path を例外に置けるままだと、file を land する前に例外だけ先に通せる＝門が空洞になる
    // （lens 2026-09-11 MEDIUM・存在しない path を 4 件足しても違反 0 だった）。
    for path in allow {
        if !listed.iter().any(|file| &file.rel == path) {
            violations.push(format!(
                "non-rust-exec: 例外行の {path} は tracked file に無い（未配線の例外は落とす）"
            ));
        }
    }
    Measured {
        fact: format!(
            "non-rust-exec={hits}/{} allow={}",
            listed.len(),
            allow.len()
        ),
        violations,
    }
}

/// CI の shell 行を数える（**検出線**・deny しない）。
///
/// 数えるのは `run:` の 1 行形と `run: |` の継続行である。閾値を持たないのは、これが
/// 「いま何行あるか」を判定行へ載せるための線だからで、増減の是非は人が読む。
pub(crate) fn ci_shell_lines(layout: &Layout) -> Measured {
    let dir = layout.root.join(".github").join("workflows");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        // workflow が無い tree（flip-check の base 複製など）は測れない側へ倒さない
        // ——検出線であって門ではないので、`n/a` を出して判定行の意味を保つ。
        return Measured {
            fact: "ci-shell-lines=n/a(no-workflows)".to_owned(),
            violations: Vec::new(),
        };
    };
    let mut total = 0_usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "yml" && ext != "yaml") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            total = total.saturating_add(count_run_lines(&text));
        }
    }
    Measured {
        fact: format!("ci-shell-lines={total}"),
        violations: Vec::new(),
    }
}

/// 1 つの workflow file の shell 行を数える。
///
/// `run: |` の継続行は **その `run:` より深い字下げが続く間**を数える（yaml parser は
/// 足さない＝依存 0 本のまま。block scalar の形はこの 1 種で足りる）。
pub(crate) fn count_run_lines(text: &str) -> usize {
    let mut total = 0_usize;
    let mut block_indent: Option<usize> = None;
    for line in text.lines() {
        let indent = line.len().saturating_sub(line.trim_start().len());
        let trimmed = line.trim_start();
        if let Some(open) = block_indent {
            if trimmed.is_empty() {
                continue;
            }
            if indent > open {
                total = total.saturating_add(1);
                continue;
            }
            block_indent = None;
        }
        let Some(rest) = trimmed.strip_prefix("run:").or_else(|| trimmed.strip_prefix("- run:"))
        else {
            continue;
        };
        let value = rest.trim();
        if value == "|" || value == ">" || value == "|-" || value == ">-" {
            block_indent = Some(indent);
            continue;
        }
        if !value.is_empty() {
            total = total.saturating_add(1);
        }
    }
    total
}
