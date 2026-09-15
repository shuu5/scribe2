//! Bash の command guard（`pre-tool-use` の 3 番目の門・設計 docs/design/vessel-hook.md §5・ADR-0025 §2.1 / §2.2・
//! SRS FR56 / FR45 / FR20 / NFR4・憲法 N1 / C1 / C2 / C14 / C16）。
//!
//! 禁じる語列は rules 行 [`ROW`]（`runner.denied_commands`・値は語列の配列）が持ち、`runner.allowed_commands`
//! と対で読む**上限側の禁止**である（vessel 宣言は緩められない・C14）。照合は 1 関数 [`denied_in`]（pure・
//! I/O なし）で、intake（`pipe::declaration` の unfit）も同じ関数を verify 行に掛ける（C2）。
//!
//! **席の弁別はしない**（runner / planner / 管理席の全 Bash に同じ判定・席ごとの例外行を持たない）。rules が読めない・
//! 行が無い・不発効の周は **deny**（FailClosed・[`POLARITY`]・読めない store を黙って通さない・NFR4）。引用符の中身・
//! 変数展開・interpreter の引数（`sh -c "…"`）は解かない（ADR-0025 §2.6・v3）。

use crate::name::NAME;
use crate::polarity::{OnFailure, Polarity, Timing};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::path::Path;

/// この境界の極性: 実行の時点で止め、禁じる語列を解けない周は Bash を通さない。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 禁じる語列を持つ rules 行の id。
pub const ROW: &str = "runner.denied_commands";

/// この門が見る tool の名。
pub const BASH: &str = "Bash";

/// segment の区切り（`;` / `&&` / `||` / `|` / 改行・`&&` と `||` は 2 文字とも区切りに数える＝間の空 segment は語を持たない）。
const SEPARATORS: &[char] = &[';', '&', '|', '\n'];

/// 当たった 1 件（当たった語列の字面・manifest の並びで最初の 1 つ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// 当たった語列（rules 行の要素の字面そのまま）。
    pub sequence: String,
}

/// command guard の判定。**bool で持たない**（憲法 C11）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandDecision {
    /// 通す（1 byte も書かない・記録も残さない）。
    Allow,
    /// 止める。`what` は記録の `what`（`command-deny ` の後ろ）・`line` は stderr へ出す 1 行。
    Deny {
        /// 記録の種別（当たった語列か、解けない理由）。
        what: String,
        /// stderr の 1 行。
        line: String,
    },
}

/// command 行が禁じる語列に当たるか（**この 1 本が唯一の判定**・pure）。
///
/// command を [`SEPARATORS`] で segment に分け、各 segment を空白で語に分け、語列の**先頭語が segment の先頭語と
/// 一致し、残りの語がすべて segment の語に含まれる**（順序不問）とき当たる。返すのは manifest の並びで最初の 1 件
/// （高々 1 つ）。
pub fn denied_in(command: &str, denied: &[String]) -> Option<Hit> {
    let segments: Vec<Vec<&str>> = command
        .split(SEPARATORS)
        .map(|segment| segment.split_whitespace().collect())
        .filter(|words: &Vec<&str>| !words.is_empty())
        .collect();
    denied
        .iter()
        .find(|sequence| {
            let mut words = sequence.split_whitespace();
            let Some(head) = words.next() else {
                return false;
            };
            let rest: Vec<&str> = words.collect();
            segments
                .iter()
                .any(|segment| segment.first() == Some(&head) && rest.iter().all(|word| segment.contains(word)))
        })
        .map(|sequence| Hit { sequence: sequence.clone() })
}

/// rules（`--rules` の差し替えか埋め込み）を読んで判定する（権能 guard と同じ経路）。読めない周は deny。
pub fn decide(command: &str, rules: Option<&Path>) -> CommandDecision {
    match rules.map_or_else(Manifest::embedded, Manifest::load) {
        Ok(manifest) => judge(command, &manifest),
        Err(_) => refused("rules-unreadable"),
    }
}

/// manifest の行だけから判定する（pure・file を撃たない）。行が無い・不発効・値が列でない周は deny。
pub fn judge(command: &str, manifest: &Manifest) -> CommandDecision {
    let Some(denied) = denied_of(manifest) else {
        return refused(&format!("no-row {ROW}"));
    };
    match denied_in(command, &denied) {
        Some(hit) => CommandDecision::Deny { line: denied_line(&hit), what: hit.sequence },
        None => CommandDecision::Allow,
    }
}

/// 行の値（無い・不発効・列でない周は `None`）。
fn denied_of(manifest: &Manifest) -> Option<Vec<String>> {
    let row = manifest.get(ROW).filter(|row| row.enabled)?;
    match row.value {
        RuleValue::List(ref sequences) => Some(sequences.clone()),
        _ => None,
    }
}

/// deny 文: **rules 行 id と当たった語列と次の一手**を 1 行で（ADR-0025 §2.2・字面は設計 vessel-hook.md §5 が正本）。
fn denied_line(hit: &Hit) -> String {
    format!(
        "{NAME}: deny {} は rules 行 {ROW} が禁じる（N1 / C16）— 契約の手順（1 行当ての A/B）か別の形に書き直す",
        hit.sequence
    )
}

/// 禁じる語列を解けない周の deny（FailClosed・理由の 1 語つき・権能 guard の断りと同じ形）。
fn refused(reason: &str) -> CommandDecision {
    CommandDecision::Deny {
        what: format!("reason={reason}"),
        line: format!("{NAME}: deny Bash は rules 行 {ROW} を読めない reason={reason}（禁じる語列を解けない周は通さない・N1 / C16）"),
    }
}

#[cfg(test)]
mod tests {
    use super::{denied_in, judge, CommandDecision, Hit, ROW};
    use crate::name::NAME;
    use crate::rules::manifest::Manifest;
    use proptest::prelude::*;
    use proptest::test_runner::Config;

    /// 反例の永続化を切り、case 数を 256 に pin する（`tests/e2e/prop.rs` と同じ形）。
    fn config() -> Config {
        Config { cases: 256, failure_persistence: None, ..Config::default() }
    }

    /// ADR-0025 §2.1 の初期値と同じ形の語列。
    fn denied() -> Vec<String> {
        ["cargo mutants", "cargo publish", "git push --force", "git push -f", "git reset --hard", "git branch -D"]
            .iter()
            .map(|item| (*item).to_owned())
            .collect()
    }

    /// `runner.denied_commands` の行を 1 つ持つ manifest（`enabled` は引数）。
    fn manifest_with(enabled: bool) -> Manifest {
        Manifest::parse(&format!(
            "schema = 1\n\n[[rule]]\nid = \"{ROW}\"\nkind = \"RunnerDeniedCommands\"\nvalue = [\"git push --force\", \"cargo mutants\"]\nenabled = {enabled}\nruling = \"r\"\nruled_at = \"d\"\n"
        ))
        .unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
    }

    /// 語列は**先頭語一致 + 残りの語の包含（順序不問）**で当たる: `git push origin main --force` と
    /// `git push --force origin main` が同じ 1 語列に当たり、`cargo nextest` / `git push origin feat/x` は当たらない。
    #[test]
    fn hook_command_sequence_matches_regardless_of_flag_order() {
        let force = Some(Hit { sequence: "git push --force".to_owned() });
        assert_eq!(denied_in("git push origin main --force", &denied()), force, "flag が末尾");
        assert_eq!(denied_in("git push --force origin main", &denied()), force, "flag が先頭");
        assert_eq!(denied_in("git push -f origin main", &denied()), Some(Hit { sequence: "git push -f".to_owned() }));
        assert_eq!(denied_in("cargo mutants --in-diff x", &denied()), Some(Hit { sequence: "cargo mutants".to_owned() }));
        for silent in [
            "cargo nextest run -p x",
            "git push origin feat/x",
            "git push --force-with-lease origin feat/x",
            "git branch -d x",
            "echo git push --force",
            "",
            "   ",
        ] {
            assert_eq!(denied_in(silent, &denied()), None, "当たらない: {silent:?}");
        }
    }

    /// 連結（`;` / `&&` / `||` / `|` / 改行）の**後ろの segment** も見る（先頭 segment だけを見ると `echo x; git push --force`
    /// が通る）。先頭語は segment ごと（`echo git push --force` は echo の segment＝当たらない）。
    #[test]
    fn hook_command_looks_at_every_segment() {
        let force = Some(Hit { sequence: "git push --force".to_owned() });
        for line in [
            "echo x; git push --force origin main",
            "cargo build && git push --force origin main",
            "cargo build || git push --force origin main",
            "true | git push --force origin main",
            "cargo build\ngit push --force origin main",
        ] {
            assert_eq!(denied_in(line, &denied()), force, "{line:?}");
        }
        assert_eq!(denied_in("git push origin main; echo --force", &denied()), None, "語は segment を跨がない");
    }

    /// 判定は行の値だけを読む: 当たれば deny（deny 文は器の名乗り・行 id・語列・次の一手）・当たらなければ Allow・
    /// 行が無い / 不発効なら deny（FailClosed・理由を名指す）。
    #[test]
    fn hook_command_judge_reads_the_row_and_fails_closed_without_it() {
        let manifest = manifest_with(true);
        let CommandDecision::Deny { what, line } = judge("git push --force origin main", &manifest) else {
            panic!("当たる command は deny");
        };
        assert_eq!(what, "git push --force", "記録の what は当たった語列");
        assert!(line.starts_with(&format!("{NAME}: deny git push --force は rules 行 {ROW} が禁じる")), "{line}");
        assert!(line.contains("N1 / C16") && line.contains("書き直す"), "次の一手を含む: {line}");
        assert_eq!(line.lines().count(), 1, "1 行: {line}");
        assert_eq!(judge("cargo nextest run -p x", &manifest), CommandDecision::Allow);
        let empty = Manifest::parse("schema = 1\n").unwrap_or_else(|errors| panic!("{errors:?}"));
        for (manifest, why) in [(&empty, "行が無い"), (&manifest_with(false), "不発効")] {
            let CommandDecision::Deny { what, line } = judge("ls", manifest) else {
                panic!("{why}: 解けない周は deny");
            };
            assert_eq!(what, format!("reason=no-row {ROW}"), "{why}");
            assert!(line.contains(&format!("reason=no-row {ROW}")), "{why}: {line}");
        }
    }

    /// 語列の語か、表に無い語。
    fn word() -> impl Strategy<Value = String> {
        prop::sample::select(vec![
            "git", "cargo", "push", "reset", "branch", "mutants", "publish", "--force", "-f", "--hard", "-D", "origin",
            "main", "ls", "echo", ";", "&&", "|", "x",
        ])
        .prop_map(str::to_owned)
    }

    /// 任意の command 行（表の語の並び）。
    fn command() -> impl Strategy<Value = String> {
        prop::collection::vec(word(), 0..12).prop_map(|words| words.join(" "))
    }

    proptest! {
        #![proptest_config(config())]

        /// 任意の command は高々 1 つの Hit を持ち、Hit は表の語列そのもの。語列の語を 1 つも含まない command は None。
        #[test]
        fn prop_hook_command_hit_is_at_most_one_and_from_the_table(line in command()) {
            let table = denied();
            let found = denied_in(&line, &table);
            if let Some(ref hit) = found {
                prop_assert!(table.contains(&hit.sequence), "{line:?}: {hit:?}");
            }
            let words: Vec<&str> = line.split_whitespace().collect();
            let any_word = table.iter().any(|sequence| sequence.split_whitespace().any(|word| words.contains(&word)));
            if !any_word {
                prop_assert!(found.is_none(), "語列の語を 1 つも含まない: {line:?}");
            }
        }

        /// 当たった語列の先頭語は command のどこかの segment の先頭語であり、残りの語はその segment に在る。
        #[test]
        fn prop_hook_command_hit_head_leads_a_segment(line in command()) {
            let Some(hit) = denied_in(&line, &denied()) else {
                return Ok(());
            };
            let mut words = hit.sequence.split_whitespace();
            let head = words.next().unwrap_or_default();
            let rest: Vec<&str> = words.collect();
            let led = line.split([';', '&', '|', '\n']).any(|segment| {
                let found: Vec<&str> = segment.split_whitespace().collect();
                found.first() == Some(&head) && rest.iter().all(|word| found.contains(word))
            });
            prop_assert!(led, "{line:?}: {hit:?}");
        }
    }
}
