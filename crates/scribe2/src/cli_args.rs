//! 引数の閉包の reader（設計 docs/design/pipeline.md §14・契約表の行 f・NFR4 / FR12）。
//!
//! 面の入口（`pipe` の dispatch・headless の分岐・`fleet` / `seat` / `vessel` / `account` の cli）が subcommand ごとの
//! `allowed`（宣言順の const 配列）を持ち、[`parse`] を 1 回撃つ。argv 全体が既知の集合に閉じているかを見る口はここ
//! 1 本で、通った argv は各面の reader が従来どおり位置で読む。`--help` / `-h` は `allowed` に無くても
//! [`ArgsError::Help`] で返り、呼び手は usage を出して rc 0 で終わる。未知・値欠け・重複は [`refusal`] の同じ口から
//! usage を添えて rc 2 で断る（state も ref も 1 本も動かさない＝入口で返る）。
//!
//! **env を読まない**（C2.2）。依存を足さない。

use crate::cli_outcome::{Outcome, RC_BROKEN};

/// usage を出して rc 0 で終わる flag（`allowed` に無くても受ける）。
pub const HELP_FLAGS: [&str; 2] = ["--help", "-h"];

/// 使い方の誤り（未知・値欠け・重複）の rc。
pub const RC_USAGE: u8 = RC_BROKEN;

/// flag が値を取るか（閉じた語・宣言順）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Takes {
    /// 値を 1 つ取る（2 回目は [`ArgsError::Duplicate`]）。
    Value,
    /// 値を 1 つ取り、何度でも現れてよい（`fleet select --exclude` の形）。
    Values,
    /// 値を取らない（在るかだけを見る・重なりは本体が裁く）。
    Switch,
    /// 値の形の token が続けば値に取り、続かなければ値なしで在る（何度でも・値の形と本数は本体が裁く＝
    /// `seat <label> -r ID` の形）。
    Maybe,
}

/// 受ける flag の 1 つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Allowed {
    /// flag の字面（`--run` / `-c` の形）。
    pub name: &'static str,
    /// 値を取るか。
    pub takes: Takes,
}

impl Allowed {
    /// 値を 1 つ取る flag。
    pub const fn value(name: &'static str) -> Self {
        Self { name, takes: Takes::Value }
    }

    /// 値を 1 つ取り、何度でも現れてよい flag。
    pub const fn values(name: &'static str) -> Self {
        Self { name, takes: Takes::Values }
    }

    /// 値を取らない flag。
    pub const fn switch(name: &'static str) -> Self {
        Self { name, takes: Takes::Switch }
    }

    /// 値を取れれば取る flag。
    pub const fn maybe(name: &'static str) -> Self {
        Self { name, takes: Takes::Maybe }
    }
}

/// 閉包の断り（閉じた enum・宣言順は [`ArgsError::as_str`] の順）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgsError {
    /// `--help` / `-h`（usage を出して rc 0）。
    Help,
    /// `allowed` に無い flag（字面つき）。
    Unknown(String),
    /// 値を取る flag に値が無い・必須の flag が無い（flag の字面つき）。
    Missing(String),
    /// 値を 1 つだけ取る flag が 2 回以上在る（flag の字面つき）。
    Duplicate(String),
}

impl ArgsError {
    /// 断りの 1 語。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Help => "help",
            Self::Unknown(_) => "unknown",
            Self::Missing(_) => "missing",
            Self::Duplicate(_) => "duplicate",
        }
    }

    /// 断りの理由の 1 行（面の前置きは呼び手が足す・値欠けと重複の字面は各面の reader と同じ）。
    pub fn reason(&self) -> String {
        match self {
            Self::Help => "usage".to_owned(),
            Self::Unknown(found) => format!("未知の引数 {found}"),
            Self::Missing(name) => format!("{name} に値が無い"),
            Self::Duplicate(name) => format!("{name} が 2 回以上在る"),
        }
    }
}

/// [`parse`] が通した argv（名指しの flag の値・在る switch・positional の列）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parsed<'a> {
    /// 値を取る flag と値（出現順）。
    values: Vec<(&'static str, &'a str)>,
    /// 在った switch（出現順）。
    switches: Vec<&'static str>,
    /// flag でも flag の値でもない token（出現順）。
    positionals: Vec<&'a str>,
}

impl<'a> Parsed<'a> {
    /// 名指しの flag の値（無ければ `None`・[`Takes::Values`] は最初の 1 つ）。
    pub fn value(&self, name: &str) -> Option<&'a str> {
        self.values.iter().find(|(found, _)| *found == name).map(|(_, value)| *value)
    }

    /// 名指しの flag の値の全部（出現順）。
    pub fn values(&self, name: &str) -> Vec<&'a str> {
        self.values.iter().filter(|(found, _)| *found == name).map(|(_, value)| *value).collect()
    }

    /// 必須の flag の値（無ければ [`ArgsError::Missing`]）。
    pub fn need(&self, name: &str) -> Result<&'a str, ArgsError> {
        self.value(name).ok_or_else(|| ArgsError::Missing(name.to_owned()))
    }

    /// switch が在るか。
    pub fn present(&self, name: &str) -> bool {
        self.switches.contains(&name)
    }

    /// positional の列。
    pub fn positionals(&self) -> &[&'a str] {
        &self.positionals
    }
}

/// flag の形の token か（`-` 1 字だけは positional）。
fn is_flag(token: &str) -> bool {
    token.len() > 1 && token.starts_with('-')
}

/// flag の値に取れる token か（各面の reader と同じ形＝`--` 始まりは値でなく次の flag）。
fn is_value(token: &str) -> bool {
    !token.starts_with("--")
}

/// argv が `allowed` に閉じているかを見て、名指しの flag の値と positional の列に分ける。
///
/// `--help` / `-h` が flag の位置に在れば他の断りより先に [`ArgsError::Help`]。それ以外は最初の断り 1 つ
/// （[`ArgsError::Unknown`] / [`ArgsError::Missing`] / [`ArgsError::Duplicate`]）で `Err`。値を取る flag は次の
/// token を値に取る（`--` 始まりは値に取らない＝値欠け）。
pub fn parse<'a>(args: &'a [String], allowed: &[Allowed]) -> Result<Parsed<'a>, ArgsError> {
    let mut parsed = Parsed::default();
    let mut first: Option<ArgsError> = None;
    let mut rest = args.iter();
    while let Some(token) = rest.next() {
        let token = token.as_str();
        if HELP_FLAGS.contains(&token) {
            return Err(ArgsError::Help);
        }
        if !is_flag(token) {
            parsed.positionals.push(token);
            continue;
        }
        let Some(spec) = allowed.iter().find(|spec| spec.name == token) else {
            first.get_or_insert_with(|| ArgsError::Unknown(token.to_owned()));
            continue;
        };
        if spec.takes == Takes::Switch {
            parsed.switches.push(spec.name);
            continue;
        }
        match (rest.clone().next().filter(|next| is_value(next)), spec.takes) {
            (Some(value), _) => {
                rest.next();
                if spec.takes == Takes::Value && parsed.value(spec.name).is_some() {
                    first.get_or_insert_with(|| ArgsError::Duplicate(spec.name.to_owned()));
                }
                parsed.values.push((spec.name, value.as_str()));
            }
            (None, Takes::Maybe) => parsed.switches.push(spec.name),
            (None, Takes::Value | Takes::Values | Takes::Switch) => {
                first.get_or_insert_with(|| ArgsError::Missing(spec.name.to_owned()));
            }
        }
    }
    match first {
        Some(error) => Err(error),
        None => Ok(parsed),
    }
}

/// 断りを outcome へ写す口（面の入口の全部が同じ 1 本）: [`ArgsError::Help`] は usage を stdout へ出して rc 0、
/// 他は `<面>: <理由>` の 1 行と usage を stderr へ出して [`RC_USAGE`]。
pub fn refusal(surface: &str, error: &ArgsError, usage: String) -> Outcome {
    match error {
        ArgsError::Help => Outcome::ok(vec![usage]),
        ArgsError::Unknown(_) | ArgsError::Missing(_) | ArgsError::Duplicate(_) => {
            Outcome::failed(RC_USAGE, vec![format!("{surface}: {}", error.reason()), usage])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, refusal, Allowed, ArgsError, RC_USAGE};
    use crate::cli_outcome::RC_OK;

    /// 歯の allowed（値・値の列・switch の 3 形）。
    const ALLOWED: &[Allowed] = &[Allowed::value("--run"), Allowed::values("--exclude"), Allowed::switch("--drive")];

    fn argv(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|item| (*item).to_owned()).collect()
    }

    /// 閉じた 4 値の as_str は宣言順（help → unknown → missing → duplicate）で、互いに異なる。
    #[test]
    fn cli_args_error_words_follow_declaration_order() {
        let all = [
            ArgsError::Help,
            ArgsError::Unknown("--x".to_owned()),
            ArgsError::Missing("--x".to_owned()),
            ArgsError::Duplicate("--x".to_owned()),
        ];
        let words: Vec<&str> = all.iter().map(ArgsError::as_str).collect();
        assert_eq!(words, ["help", "unknown", "missing", "duplicate"], "宣言順の as_str");
    }

    /// 閉じた argv は値・値の列・switch・positional に分かれる。
    #[test]
    fn cli_args_closed_argv_splits_values_switches_and_positionals() {
        let args = argv(&["hold", "--run", "r1", "--exclude", "a", "--drive", "--exclude", "b", "B-1"]);
        let parsed = parse(&args, ALLOWED).expect("閉じた argv は通る");
        assert_eq!(parsed.value("--run"), Some("r1"));
        assert_eq!(parsed.need("--run"), Ok("r1"));
        assert_eq!(parsed.values("--exclude"), ["a", "b"], "値の列は出現順");
        assert!(parsed.present("--drive"));
        assert_eq!(parsed.positionals(), ["hold", "B-1"]);
        assert_eq!(parsed.need("--nope"), Err(ArgsError::Missing("--nope".to_owned())), "必須の欠けは Missing");
        assert!(parse(&[], ALLOWED).is_ok_and(|found| found.positionals().is_empty()), "空の argv は通る");
    }

    /// `--help` / `-h` は allowed に無くても Help・他の断りより先に返る（値の位置の `-h` は値）。
    #[test]
    fn cli_args_help_wins_even_outside_allowed() {
        for raw in [&["--help"][..], &["-h"], &["--bogus", "--help"], &["--run", "--help"], &["--run", "r1", "--run", "r2", "-h"]] {
            assert_eq!(parse(&argv(raw), ALLOWED), Err(ArgsError::Help), "{raw:?}");
        }
        let taken = argv(&["--run", "-h"]);
        assert_eq!(parse(&taken, ALLOWED).map(|found| found.value("--run")), Ok(Some("-h")), "値の位置の -h は値");
    }

    /// 未知・値欠け・重複は最初の 1 つで typed に断る（Values と Switch の重なりは断らない）。
    #[test]
    fn cli_args_refuses_unknown_missing_and_duplicate_typed() {
        let cases: [(&[&str], ArgsError); 6] = [
            (&["--bogus"], ArgsError::Unknown("--bogus".to_owned())),
            (&["--run", "r1", "-x"], ArgsError::Unknown("-x".to_owned())),
            (&["--run"], ArgsError::Missing("--run".to_owned())),
            (&["--run", "--drive"], ArgsError::Missing("--run".to_owned())),
            (&["--run", "r1", "--run", "r2"], ArgsError::Duplicate("--run".to_owned())),
            (&["--bogus", "--run"], ArgsError::Unknown("--bogus".to_owned())),
        ];
        for (raw, want) in cases {
            assert_eq!(parse(&argv(raw), ALLOWED), Err(want), "{raw:?}");
        }
        assert!(parse(&argv(&["--drive", "--drive", "--exclude", "a", "--exclude", "a"]), ALLOWED).is_ok(), "Switch と Values の重なりは通す");
    }

    /// Maybe は値の形の token（`--` 始まりでない＝`-` 1 字始まりも値）が続けば値に取り、続かなければ値なしで在る
    /// （何度でも・断らない）。
    #[test]
    fn cli_args_maybe_takes_a_value_only_when_one_follows() {
        let allowed = [Allowed::maybe("-r"), Allowed::switch("-c"), Allowed::switch("--drive")];
        let args = argv(&["-r", "-f3c9", "-c", "-r", "--drive", "-r"]);
        let parsed = parse(&args, &allowed).expect("Maybe の欠けと重なりは通す");
        assert_eq!(parsed.values("-r"), ["-f3c9"], "値の形の token だけを値に取る");
        assert!(parsed.present("-r") && parsed.present("-c") && parsed.present("--drive"), "値なしの -r は在る");
        assert!(parsed.positionals().is_empty());
        assert_eq!(parse(&argv(&["-r", "--bogus"]), &allowed), Err(ArgsError::Unknown("--bogus".to_owned())), "続く flag は値に取らない");
    }

    /// 断りの口: Help は usage を stdout に rc 0・他は理由の 1 行と usage を stderr に rc 2。
    #[test]
    fn cli_args_refusal_maps_help_to_rc0_and_the_rest_to_rc2() {
        let help = refusal("toy", &ArgsError::Help, "usage: toy".to_owned());
        assert_eq!((help.rc, help.out, help.err.len()), (RC_OK, vec!["usage: toy".to_owned()], 0));
        let unknown = refusal("toy", &ArgsError::Unknown("--bogus".to_owned()), "usage: toy".to_owned());
        assert_eq!(unknown.rc, RC_USAGE);
        assert_eq!(unknown.err, ["toy: 未知の引数 --bogus", "usage: toy"]);
        assert!(unknown.out.is_empty());
        for error in [ArgsError::Missing("--run".to_owned()), ArgsError::Duplicate("--run".to_owned())] {
            let refused = refusal("toy", &error, "usage: toy".to_owned());
            assert_eq!((refused.rc, refused.err.last().map(String::as_str)), (RC_USAGE, Some("usage: toy")), "{error:?}");
        }
        assert_eq!(RC_USAGE, 2, "使い方の誤りは rc 2");
    }
}
