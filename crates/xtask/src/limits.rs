//! `cargo xtask check` が測る閾値と lint 集合の唯一の置き場。
//!
//! 数値を .rs の各所へ散らさないための const 置き場である。rules manifest への
//! 移設は leg 2 の所管なので、本 leg ではここが SSOT でよい。test 側の期待値も
//! ここを参照して機械的に作る（fixture に magic number を書かない）。

/// core crate の `src` 配下 `.rs` の総行数の上限。
pub const MAX_CORE_LINES: usize = 40_000;

/// `crates/*/src` 配下 `.rs` 1 file あたりの物理行数の上限。
pub const MAX_FILE_LINES: usize = 1_500;

/// workspace root の `Cargo.toml` が持つべき lint の 3 つ組
/// `(section, lint 名, level)`。section は `workspace.lints.<section>` の後半。
///
/// 個数は契約に書かない（この配列が唯一の SSOT である）。`print_stdout` /
/// `print_stderr` を deny に据え置くのは、出力層に `#[expect]` を使えなくなる
/// forbid を避けるためである。逆に粒度 lint 3 本は `#[allow]` / `#[expect]`
/// による黙殺を compile error にしたいので forbid とする。
pub const REQUIRED_LINTS: &[(&str, &str, &str)] = &[
    ("rust", "unsafe_code", "forbid"),
    ("rust", "unused_must_use", "deny"),
    ("clippy", "unwrap_used", "deny"),
    ("clippy", "expect_used", "deny"),
    ("clippy", "panic", "deny"),
    ("clippy", "todo", "deny"),
    ("clippy", "unimplemented", "deny"),
    ("clippy", "unreachable", "deny"),
    ("clippy", "exit", "deny"),
    ("clippy", "indexing_slicing", "deny"),
    ("clippy", "dbg_macro", "deny"),
    ("clippy", "print_stdout", "deny"),
    ("clippy", "print_stderr", "deny"),
    ("clippy", "too_many_arguments", "forbid"),
    ("clippy", "too_many_lines", "forbid"),
    ("clippy", "cognitive_complexity", "forbid"),
    ("clippy", "allow_attributes", "deny"),
];

/// tracked file の本文に残してはならない private path 形の needle 集合。
///
/// 集合は 2 形ちょうどである（絶対 home dir の接頭形と、home dir の短縮展開記号 +
/// 区切りの 2 byte 形）。字面をこの file に置くと paths-clean が自分自身を撃つので
/// **実行時に組み立てる**（`concat!` は compile 時に連結するため source の byte 列に
/// needle が現れない）。行頭錨ではなく行中のどこに現れても違反である。
pub const PRIVATE_PATH_MARKS: &[&str] = &[concat!("/", "home", "/"), concat!("~", "/")];

/// `[dependencies]` / `[dev-dependencies]` に在ってよい依存の `(section, dep 名)`。
///
/// crate 名の字面を持たない 2 つ組であるため、同名の dep をどの crate が宣言しても
/// 区別できない（xtask に同名 dev-dep を足しても検出できないのは既知の限界であり、
/// crate 粒度の回復は leg 2 の所管である）。
pub const ALLOWED_DEPS: &[(&str, &str)] = &[
    ("dev-dependencies", "insta"),
    ("dev-dependencies", "proptest"),
];

#[cfg(test)]
mod tests {
    use super::{MAX_CORE_LINES, MAX_FILE_LINES};
    use crate::toml_lite::{quoted, sections};
    use std::path::PathBuf;

    /// `[[rule]]` の section header を [`sections`] が返す字面。
    ///
    /// `sections` は `[` を 1 つだけ剥がすので、array-of-tables は `[rule` になる。
    const RULE_HEADER: &str = "[rule";

    /// manifest の本文（workspace root は この crate の 2 つ上）。
    fn manifest_text() -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("rules")
            .join("manifest.toml");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{} を読めない: {err}", path.display()))
    }

    /// 行 id を持つ `[[rule]]` の `field` の生の値を引く。
    fn raw_field(text: &str, id: &str, field: &str) -> Option<String> {
        for (header, pairs) in sections(text) {
            if header != RULE_HEADER {
                continue;
            }
            let found = pairs
                .iter()
                .find(|(key, _)| *key == "id")
                .and_then(|(_, value)| quoted(value));
            if found.as_deref() != Some(id) {
                continue;
            }
            return pairs
                .iter()
                .find(|(key, _)| *key == field)
                .map(|(_, value)| (*value).to_owned());
        }
        None
    }

    /// 行 id を持つ `[[rule]]` の `value` を整数で引く。
    fn int_value(text: &str, id: &str) -> Option<u64> {
        raw_field(text, id, "value").and_then(|value| value.trim().parse::<u64>().ok())
    }

    /// 行 id を持つ `[[rule]]` の `ruling` を引く。
    fn ruling(text: &str, id: &str) -> Option<String> {
        raw_field(text, id, "ruling").and_then(|value| quoted(&value))
    }

    /// `limits.rs` の const と manifest の行が同じ値である（憲法 C14.2 の最小形）。
    ///
    /// const の manifest 移設は後続の便なので、いまは 2 面の写しの一致を歯で守る。
    #[test]
    fn limits_match_rules_manifest() {
        let text = manifest_text();
        assert_eq!(
            int_value(&text, "R-C4-1"),
            Some(MAX_CORE_LINES as u64),
            "R-C4-1 と MAX_CORE_LINES"
        );
        assert_eq!(
            int_value(&text, "R-C4-2"),
            Some(MAX_FILE_LINES as u64),
            "R-C4-2 と MAX_FILE_LINES"
        );
    }

    /// 憲法 §3 の閾値セル（写し）を持つ file。
    fn constitution_text() -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("design-intent")
            .join("spec")
            .join("constitution.html");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{} を読めない: {err}", path.display()))
    }

    /// `<tr id="…">` の 4 列目（初期値）を tag を剥がして返す。
    ///
    /// **無い行を黙って飛ばさない**——行が消えた周に「一致した」と言わせないためで、
    /// 見つからない / 列が足りないはいずれも panic（測れなかったを緑にしない）。
    fn initial_cell(html: &str, id: &str) -> String {
        let head = format!("<tr id=\"{id}\">");
        // **最初の 1 件で引かない**。元の行をコメントで殺して生きた行を書き換える形と、
        // 同じ id の行を 2 本目として足す形は、どちらも「先頭の 1 件」だけを見る歯を
        // 素通りする（実測: どちらも rc 0 で、描画される §3 の値だけが変わる）。
        let hits = html.matches(&head).count();
        assert_eq!(hits, 1, "§3 の {id} の行が 1 本でない");
        let at = html
            .find(&head)
            .unwrap_or_else(|| panic!("§3 に {id} の行が無い"));
        // 行の終わりは**次の `<tr` の手前**までで測る。`</tr>` を全文から探すと、
        // 当該行の閉じ tag が消えた周に**次の行の `</tr>`** で止まって緑になる
        // （表が壊れているのに「測れた」と言う形＝N2）。
        let rest = &html[at..];
        let limit = rest[head.len()..]
            .find("<tr")
            .map_or(rest.len(), |at_next| at_next + head.len());
        let row = &rest[..limit];
        let end = row
            .find("</tr>")
            .unwrap_or_else(|| panic!("{id} の行が閉じていない"));
        let cells: Vec<&str> = row[..end].split("<td>").skip(1).collect();
        let cell = cells
            .get(3)
            .unwrap_or_else(|| panic!("{id} に 4 列目（初期値）が無い"));
        strip_tags(&drop_deleted(cell))
    }

    /// 改訂 marker の**削除側**（`<del …>…</del>`）を本文ごと落とす。
    ///
    /// 憲法の改訂形は `<del class="delta">旧</del><ins class="delta">新</ins>` の対である
    /// （N4.2）。tag を剥がすだけだと旧と新の数字が両方残り、**manifest と整合した合法な
    /// 改訂が RED になる**（実測: `left: 7 right: 6`）。生きている値は `<ins>` 側なので、
    /// 削除側は本文ごと落として読む。
    fn drop_deleted(cell: &str) -> String {
        let mut out = String::new();
        let mut rest = cell;
        while let Some(at) = rest.find("<del") {
            out.push_str(&rest[..at]);
            let tail = &rest[at..];
            match tail.find("</del>") {
                Some(end) => rest = &tail[end.saturating_add("</del>".len())..],
                None => return out,
            }
        }
        out.push_str(rest);
        out
    }

    /// tag を剥がし、桁区切りの `,` を落とした本文（parser は書かない・std だけ）。
    fn strip_tags(cell: &str) -> String {
        let mut out = String::new();
        let mut inside = false;
        for ch in cell.chars() {
            match ch {
                '<' => inside = true,
                '>' => inside = false,
                ',' => {}
                _ if !inside => out.push(ch),
                _ => {}
            }
        }
        out
    }

    /// 本文の整数を**出現順**に拾う。
    fn ints(body: &str) -> Vec<u64> {
        let mut found = Vec::new();
        let mut digits = String::new();
        for ch in body.chars().chain(std::iter::once(' ')) {
            if ch.is_ascii_digit() {
                digits.push(ch);
                continue;
            }
            if !digits.is_empty() {
                let parsed = digits
                    .parse::<u64>()
                    .unwrap_or_else(|err| panic!("{digits} を整数にできない: {err}"));
                found.push(parsed);
                digits.clear();
            }
        }
        found
    }

    /// 数値 token（`0-9` と `.` の連なり）を出現順に拾う。
    fn number_tokens(body: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut token = String::new();
        for ch in body.chars().chain(std::iter::once(' ')) {
            if ch.is_ascii_digit() || (ch == '.' && !token.is_empty()) {
                token.push(ch);
                continue;
            }
            if !token.is_empty() {
                found.push(std::mem::take(&mut token));
            }
        }
        found
    }

    /// 「1.0」形の比を pct（×100）で読む。小数点以下は 2 桁までを許す。
    ///
    /// **個数も測る**——最初の token だけを読む形だと、セルに数値を 1 つ足す手編集
    /// （`1.0 以下 (2 面で測る)`）が緑で通る（他の 3 行は個数違いを FAIL にしている）。
    fn pct(body: &str) -> u64 {
        let tokens = number_tokens(body);
        assert_eq!(tokens.len(), 1, "比のセルの数値が 1 個でない: {tokens:?}");
        let token = tokens.first().cloned().unwrap_or_default();
        let (whole, fraction) = match token.split_once('.') {
            Some((left, right)) => (left.to_owned(), right.to_owned()),
            None => (token.clone(), String::new()),
        };
        assert!(fraction.len() <= 2, "小数点以下が 2 桁を超える: {token}");
        let padded = format!("{fraction:0<2}");
        let hundreds = ints(&whole).first().copied().unwrap_or_else(|| panic!("比を読めない: {body}"));
        let rest = ints(&padded).first().copied().unwrap_or(0);
        hundreds.saturating_mul(100).saturating_add(rest)
    }

    /// 憲法 §3 の初期値の裁定 id の接頭辞。§3 の閾値セルはこの裁定の値を写した列である。
    const INITIAL_RULING: &str = "RULING-v2-p1-exit-bundle";

    /// §3 の閾値セルと突合する manifest の行（セルから拾う数値の出現順）。
    const THRESHOLD_ROWS: [&str; 6] = [
        "R-C4-1",
        "R-C4-2",
        "R-C4-3",
        "R-C4-4.fn-lines",
        "R-C4-4.complexity",
        "R-C4-4.args",
    ];

    /// §3 の閾値セル（初期値）と manifest の行を突合し、**裁定の無い差**を返す。
    ///
    /// 現在値の正本は manifest の行で、§3 は初期値の列である。初期値と違う値の行は、
    /// ruling が初期の裁定（[`INITIAL_RULING`]）でない＝新しい裁定で改めた行だけを通す。
    /// 値を変えて ruling を初期のまま残した行（手編集）は差として返す。
    fn unruled_drift(html: &str, text: &str) -> Vec<String> {
        let mut cells: Vec<u64> = Vec::new();
        cells.extend(ints(&initial_cell(html, "r-c4-1")));
        cells.extend(ints(&initial_cell(html, "r-c4-2")));
        cells.push(pct(&initial_cell(html, "r-c4-3")));
        cells.extend(ints(&initial_cell(html, "r-c4-4")));
        assert_eq!(cells.len(), THRESHOLD_ROWS.len(), "§3 の閾値セルから拾えた数値: {cells:?}");
        let mut drift = Vec::new();
        for (cell, id) in cells.iter().zip(THRESHOLD_ROWS) {
            let value = int_value(text, id).unwrap_or_else(|| panic!("manifest に {id} が無い"));
            if value == *cell {
                continue;
            }
            let ruled = ruling(text, id).unwrap_or_else(|| panic!("manifest の {id} に ruling が無い"));
            if ruled.starts_with(INITIAL_RULING) {
                drift.push(format!("{id}: §3 = {cell}・manifest = {value}・ruling = {ruled}"));
            }
        }
        drift
    }

    /// 憲法 §3 の閾値セルは manifest の初期値の写しである（裁定なしの手編集は RED）。
    ///
    /// 値の正本は manifest 側で、憲法は読む人のための初期値の列である。3 面目のこの写しを
    /// 手で書き換えても self-test も folio も緑のままだったので、ここで突合する。
    // flip-check: retroactive s2-07l.8
    #[test]
    fn constitution_thresholds_match_rules_manifest() {
        let drift = unruled_drift(&constitution_text(), &manifest_text());
        assert!(drift.is_empty(), "憲法 §3 の閾値セルと rules manifest の値: {drift:?}");
    }

    /// §3 の 4 行の fixture（初期値 = 20,000 / 1,500 / 1.0 / 60・15・5）。
    const HTML_FIXTURE: &str = concat!(
        "<tr id=\"r-c4-1\"><td>R-C4-1</td><td>C4</td><td>core</td><td>20,000 行 以下</td></tr>\n",
        "<tr id=\"r-c4-2\"><td>R-C4-2</td><td>C4</td><td>module</td><td>1,500 行 以下</td></tr>\n",
        "<tr id=\"r-c4-3\"><td>R-C4-3</td><td>C4</td><td>ratio</td><td>1.0 以下</td></tr>\n",
        "<tr id=\"r-c4-4\"><td>R-C4-4</td><td>C4</td><td>fn</td><td>関数 60 行・複雑度 15・引数 5</td></tr>\n",
    );

    /// 6 行の manifest fixture。R-C4-1 だけ値と ruling を差し替える。
    fn manifest_fixture(core_lines: u64, core_ruling: &str) -> String {
        let rows = [
            ("R-C4-1", core_lines, core_ruling),
            ("R-C4-2", 1500, "RULING-v2-p1-exit-bundle 論点 2"),
            ("R-C4-3", 100, "RULING-v2-p1-exit-bundle 論点 2"),
            ("R-C4-4.fn-lines", 60, "RULING-v2-p1-exit-bundle 論点 2"),
            ("R-C4-4.complexity", 15, "RULING-v2-p1-exit-bundle 論点 2"),
            ("R-C4-4.args", 5, "RULING-v2-p1-exit-bundle 論点 2"),
        ];
        let mut text = "schema = 1\n".to_owned();
        for (id, value, ruled) in rows {
            text.push_str(&format!(
                "\n[[rule]]\nid = \"{id}\"\nvalue = {value}\nruling = \"{ruled}\"\n"
            ));
        }
        text
    }

    /// 値を変え ruling を新しい裁定にした行は通る（初期値のままの行も通る）。
    // flip-check: retroactive s2-07l.191
    #[test]
    fn constitution_drift_passes_rows_changed_under_a_new_ruling() {
        let initial = manifest_fixture(20_000, "RULING-v2-p1-exit-bundle 論点 2");
        assert_eq!(unruled_drift(HTML_FIXTURE, &initial), Vec::<String>::new(), "初期値のまま");
        let ruled = manifest_fixture(26_000, "user 2026-09-13T07:08Z");
        assert_eq!(unruled_drift(HTML_FIXTURE, &ruled), Vec::<String>::new(), "新しい裁定で改めた行");
    }

    /// 値を変え ruling を初期の裁定のまま残した行（裁定なしの手編集）は差として返る。
    #[test]
    fn constitution_drift_flags_rows_changed_under_the_initial_ruling() {
        let unruled = manifest_fixture(26_000, "RULING-v2-p1-exit-bundle 論点 2");
        assert_eq!(
            unruled_drift(HTML_FIXTURE, &unruled),
            vec!["R-C4-1: §3 = 20000・manifest = 26000・ruling = RULING-v2-p1-exit-bundle 論点 2".to_owned()]
        );
    }
}
