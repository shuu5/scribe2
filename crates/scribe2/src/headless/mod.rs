//! `claude -p` を包む 2 つの薄い口（設計 docs/design/pipeline.md §6・FR5 / FR9 / NFR1）。
//!
//! **この器は env を読まない**（憲法 C2.2）。口座の切替は `--account-dir` で受けた値を
//! **子 process の環境変数へ書く**だけで、自分の環境変数を覗きに行くことはしない——
//! 「書く」と「読む」は別である。読んでしまうと、同じコマンドが host ごとに違う
//! 意味を持つ。
//!
//! prompt の文面は tracked な template file（`runner.txt` / `lens.txt`）に置く。
//! 絶対 path も口座名も host 名も書かない（本 repo は PUBLIC・`xtask check` の
//! `paths-clean` が落とす）。

#[path = "lens.rs"]
mod lens_body;
#[path = "runner.rs"]
mod runner_body;

/// 値を取る headless の flag。**重なりは閉包の断りにしない**: 本体の [`flag`] が両方の値を名乗って断る
/// （設計 account-autonomy.md §16・その字面と rc は不変）。
const fn value(name: &'static str) -> crate::cli_args::Allowed {
    crate::cli_args::Allowed::values(name)
}

/// 分岐の直後の閉包の検査（設計 pipeline.md §14 約束 5）: 未知の flag と値欠けは rc 2・`--help` は usage で rc 0・
/// 通った argv は本体へそのまま渡す（本体の reader は従来どおり位置で読む）。
fn entry(
    args: &[String],
    allowed: &[crate::cli_args::Allowed],
    surface: &str,
    usage: fn() -> String,
    body: fn(&[String]) -> crate::cli_outcome::Outcome,
) -> crate::cli_outcome::Outcome {
    match crate::cli_args::parse(args, allowed) {
        Ok(_) => body(args),
        Err(error) => crate::cli_args::refusal(surface, &error, usage()),
    }
}

/// `<NAME> lens` の口（分岐の直後に [`entry`] を 1 回撃ち、通った argv を本体へ渡す）。
pub mod lens {
    pub use super::lens_body::*;
    use super::value;
    use crate::cli_args;
    use crate::cli_outcome::Outcome;

    /// lens が受ける flag（本体の `KNOWN_FLAGS` と同じ 7 つ・宣言順）。
    const ALLOWED: &[cli_args::Allowed] = &[
        value("--contract"),
        value("--worktree"),
        value("--permission-mode"),
        value("--rules"),
        value("--account-dir"),
        value("--claude"),
        value("--cgroup-root"),
    ];

    /// `lens` を 1 回。
    pub fn dispatch(args: &[String]) -> Outcome {
        super::entry(args, ALLOWED, "lens", usage, super::lens_body::dispatch)
    }
}

/// `<NAME> runner` の口（分岐の直後に [`entry`] を 1 回撃ち、通った argv を本体へ渡す）。
pub mod runner {
    pub use super::runner_body::*;
    use super::value;
    use crate::cli_args;
    use crate::cli_outcome::Outcome;

    /// runner が受ける flag（usage の 9 つ・宣言順）。
    const ALLOWED: &[cli_args::Allowed] = &[
        value("--worktree"),
        value("--write-set"),
        value("--vessel"),
        value("--plugin-dir"),
        value("--permission-mode"),
        value("--rules"),
        value("--account-dir"),
        value("--claude"),
        value("--cgroup-root"),
    ];

    /// `runner` を 1 回。
    pub fn dispatch(args: &[String]) -> Outcome {
        super::entry(args, ALLOWED, "runner", usage, super::runner_body::dispatch)
    }
}

use crate::fleet::select::{Model, MODELS};
use crate::pipe::confine;
use crate::rules::manifest::Manifest;
use crate::rules::str_row;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// claude の scope の unit 名に載せる段の名。
const CLAUDE_STAGE: &str = "claude";

/// claude を起こしたのが runner の口か lens の口か（scope の unit 名に載る）。
///
/// 逐次 record が要るのは runner だけ（[`Format::StreamJson`]）なので、口の別はその 1 つで読める。
fn claude_place(call: &Call<'_>) -> &'static str {
    match call.output {
        Format::StreamJson => "runner",
        Format::Json | Format::Text => "lens",
    }
}

/// claude の出力形式（`--output-format` の値・**閉じた 3 値**・設計 gate-cost.md §26 形 (2)）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// 既定（text）。`--output-format` を渡さない（`fleet usage` の token refresh・argv は不変）。
    Text,
    /// 1 object の封筒（`result` の text と `usage` を持つ）。lens が使う——判定は封筒の `result` の text の
    /// 最後の JSON 行から読み、`usage` は消費の 6 値として読む。
    Json,
    /// record を**逐次**受け取る。runner が使う——rate limit の record を**途中で**見て止めるため。**全行が JSON**
    /// ゆえ「最後の JSON 行」が claude 自身の result record になり、lens の判定は record の中の文字列へ埋もれる
    /// （実測 2026-09-10）ので lens には使わない。
    StreamJson,
}

/// [`Format`] の全 variant（宣言順）。
pub const FORMATS: &[Format] = &[Format::Text, Format::Json, Format::StreamJson];

impl Format {
    /// `--output-format` の値（[`Format::Text`] は flag ごと渡さない＝`None`）。
    fn flag(self) -> Option<&'static str> {
        match self {
            Self::Text => None,
            Self::Json => Some("json"),
            Self::StreamJson => Some("stream-json"),
        }
    }
}

/// 既定の claude 実行 file。`--claude` は **test の seam**（fake の実行 file を渡す）。
pub const DEFAULT_CLAUDE: &str = "claude";

/// rate limit で止めた周の rc。呼出側（`pipe`）はこれを `Failed detail=rate-limit` に写す。
pub const RC_RATE_LIMIT: u8 = 75;

/// API に届かず止まった周の rc（設計 account-autonomy.md §17・`s2-07l.301`）。呼出側（`pipe`）は `Failed` を記帳せず、
/// 段を `Spawned` のまま `SeatStopped detail=runner-unreachable` に写す。値は [`RC_RATE_LIMIT`] と
/// `pipe::RC_QUESTION`（76）の隣で、既存の rc と衝突しない（in-file の歯が pin する）。
pub const RC_UNREACHABLE: u8 = 77;

/// 口座の切替に使う**子 process の**環境変数。ここへ書くだけで、自分では読まない。
///
/// `--account-dir` を渡さない周は、親のこの env が**そのまま子へ継承される**（planner 裁定
/// 2026-09-10 Q5）。消す形も採れるが、この env で口座を切っている環境では runner を黙って
/// 既定口座へ落とす実害があり、消すこと自体も env への介入である。憲法 C2.2 が禁じるのは
/// 「読むこと」と「新しい seam を導入すること」で、継承はそのどちらでもない。
pub const ACCOUNT_ENV: &str = "CLAUDE_CONFIG_DIR";

/// agent view を切る**子 process の**環境変数（設計 account-autonomy.md §5「agent view の前提」・`s2-07l.239`）。
///
/// [`ACCOUNT_ENV`] と同じく子へ設定するだけで、自分では読まない（C2.2）。器が起こす claude の構築点（本 module の
/// [`build`] と `seat::cycle::relaunch` の起動行）が必ず設定する＝settings に依らず構造で切れる。
pub const AGENT_VIEW_ENV: &str = "CLAUDE_CODE_DISABLE_AGENT_VIEW";

/// [`AGENT_VIEW_ENV`] に設定する値（agent view off）。
pub const AGENT_VIEW_OFF: &str = "1";

/// 判定に届かなかった周の 1 行（lens の既定）。
pub const INCONCLUSIVE_HEAD: &str = r#"{"verdict":"INCONCLUSIVE","evidence":"#;

/// 値を持たない flag の代わりに理由へ出す 1 語（[`flag`] の二重の断りと、起動行の受付
/// （`pipe::spawn::LineRefusal`）が**同じ語**で名乗る）。
pub const NO_VALUE: &str = "値なし";

/// flag の値を取る。**同名が 2 回以上在る**か値が無ければ理由つきで `Err`。
///
/// `pipe::cli` に同形の関数が在るが、そちらは private であり、pub にするには
/// `pipe` 側へ手を入れることになる（本契約の「やらない」に当たる）。**同じ形を
/// 2 つ持つより、契約の柵を守るほうを採った**。
///
/// **二重は値の読みより先に断る**（設計 account-autonomy.md §16・`s2-07l.411`）。最初の出現を採ると、
/// 器が足した `--account-dir` の後ろに散文で書かれた値（や、その逆）が黙って捨てられ、記帳した口座と
/// 実際に走る口座がずれる。どちらが正かは器に分からない（C10）ので、読み手 1 本＝ここで断る。runner /
/// lens の**全 flag**（[`need`] 経由の必須も含む）が同じ 1 経路を通る。
pub fn flag<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
    let mut seen = args.iter().enumerate().filter(|(_, arg)| arg.as_str() == name).map(|(at, _)| at);
    let Some(at) = seen.next() else {
        return Ok(None);
    };
    if let Some(again) = seen.next() {
        return Err(format!("{name} が 2 回以上在る（{} / {}）", value_at(args, at), value_at(args, again)));
    }
    match args.get(at + 1) {
        Some(found) if !found.starts_with("--") => Ok(Some(found)),
        _ => Err(format!("{name} に値が無い")),
    }
}

/// `at` の flag が持つ値（次の token・`--` 始まりと末尾は [`NO_VALUE`]）。二重の理由に
/// **両方の値**を載せるための読み（値の判定は [`flag`] と同じ形）。
fn value_at(args: &[String], at: usize) -> &str {
    match args.get(at + 1) {
        Some(found) if !found.starts_with("--") => found,
        _ => NO_VALUE,
    }
}

/// 必須の flag。
pub fn need<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    flag(args, name)?.ok_or(format!("{name} が要る"))
}

/// runner / lens が claude に毎回渡す model を持つ rules 行（設計 pipeline.md §6・`s2-07l.297`）。
pub const ROW_MODEL: &str = "runner.model";

/// 同じく effort を持つ rules 行（設計 pipeline.md §6・`s2-07l.322`）。
pub const ROW_EFFORT: &str = "runner.effort";

/// claude に**毎回**渡す effort（`--effort` の値・rules 行 [`ROW_EFFORT`] の語彙・閉じた表）。
///
/// 省くと口座の設定 dir の `settings.json`（`effortLevel`）の値で決まり、同じ便が口座ごとに違う深さで走る
/// （席の model 事故と同じ根因）。置き場が `fleet::select` でなく headless なのは、口座選定が effort を読まないため。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    /// low。
    Low,
    /// medium。
    Medium,
    /// high。
    High,
    /// xhigh。
    Xhigh,
}

/// [`Effort`] の全 variant（宣言順）。
pub const EFFORTS: &[Effort] = &[Effort::Low, Effort::Medium, Effort::High, Effort::Xhigh];

impl Effort {
    /// claude CLI の字面（`--effort` の値）。
    pub fn alias(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }

    /// 字面との**完全一致**で引く（case-fold しない）。表に無い字面は `None`。
    pub fn parse(text: &str) -> Option<Self> {
        EFFORTS.iter().copied().find(|found| found.alias() == text)
    }
}

/// `--rules PATH` が在ればその manifest・無ければ埋め込み（`pipe::cli` と同じ規約）。読めない周は理由つきで `Err`。
///
/// **runner と lens の manifest の読み口はここ 1 つ**（lens の cap と両者の model が同じ manifest から来る）。
/// runner が manifest から読むのは model の 1 行だけで、allowlist と common-verify は従来どおり便の写し
/// （`--vessel`）から読む（ADR-0010 §2.4 は動かない）。
pub fn rules_of(args: &[String]) -> Result<Manifest, String> {
    let loaded = match flag(args, "--rules")? {
        Some(path) => Manifest::load(Path::new(path)),
        None => Manifest::embedded(),
    };
    loaded.map_err(|errors| {
        let joined = errors.iter().map(ToString::to_string).collect::<Vec<String>>().join(" / ");
        format!("rules を読めない: {joined}")
    })
}

/// rules 行 [`ROW_MODEL`] の model。行が無い / 不発効 / 文字列でない / 閉じた表（[`Model::parse`]）に無い周は
/// 理由つきで `Err`＝呼び手は claude を呼ばず rc 2（cap と同じ極性・版の既定へ黙って倒れない）。
pub fn runner_model(manifest: &Manifest) -> Result<Model, String> {
    let text = str_row(manifest, ROW_MODEL)?;
    Model::parse(text).ok_or_else(|| {
        let taken: Vec<&str> = MODELS.iter().map(|model| model.alias()).collect();
        format!("{ROW_MODEL} の値 {text} は未知の model（取るのは {}）", taken.join(" / "))
    })
}

/// rules 行 [`ROW_EFFORT`] の effort。4 理由の `Err` は [`runner_model`] と同じ極性＝呼び手は claude を呼ばず rc 2。
pub fn runner_effort(manifest: &Manifest) -> Result<Effort, String> {
    let text = str_row(manifest, ROW_EFFORT)?;
    Effort::parse(text).ok_or_else(|| {
        let taken: Vec<&str> = EFFORTS.iter().map(|effort| effort.alias()).collect();
        format!("{ROW_EFFORT} の値 {text} は未知の effort（取るのは {}）", taken.join(" / "))
    })
}

/// stdin をすべて **byte のまま**読む。
///
/// diff は UTF-8 とは限らず、cap の判定は byte 数で行う（文字数に直すと、同じ diff が
/// 別の大きさを名乗る）。読めなければ空＝「渡されなかった」として扱う。
pub fn read_stdin_bytes() -> Vec<u8> {
    let mut buffer = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buffer);
    buffer
}

/// claude を 1 回起動するための材料。
pub struct Call<'a> {
    /// claude の実行 file（`--claude` の値・既定 [`DEFAULT_CLAUDE`]）。
    pub claude: &'a str,
    /// 組み上がった prompt。
    pub prompt: &'a str,
    /// **毎回明示する** permission mode（既定に頼らない＝既定は版で動く）。
    pub permission_mode: &'a str,
    /// claude に渡す model（`--model <値>`・claude CLI の別名）。runner と lens は rules 行 `runner.model` の値を
    /// **毎回**渡す（`Some`・permission mode と同じ理由＝版の既定に従うと便が消費するモデル別窓が黙って変わり、
    /// 便用の口座選定が数える窓とずれる・`s2-07l.297`）。`fleet usage` の token refresh は `None`（argv は不変）。
    pub model: Option<&'a str>,
    /// claude に渡す effort（`--effort <値>`・[`Effort::alias`]）。runner と lens は rules 行 `runner.effort` の値を
    /// **毎回**渡す（`Some`・model と同じ理由＝省くと口座ごとに深さがばらばらになる・`s2-07l.322`）。
    /// `fleet usage` の token refresh は `None`（argv は不変）。
    pub effort: Option<&'a str>,
    /// 本 repo の plugin を載せる dir。
    pub plugin_dir: Option<&'a str>,
    /// 口座の設定 dir（子の環境変数へ書く値）。
    pub account_dir: Option<&'a str>,
    /// 起動する cwd。
    pub cwd: Option<&'a Path>,
    /// 出力形式（[`Format`] の 3 値）。runner は [`Format::StreamJson`]（rate limit の record を途中で見る）・lens は
    /// [`Format::Json`]（判定と消費の 6 値を 1 object で受ける）・`fleet usage` の token refresh は [`Format::Text`]。
    pub output: Format,
    /// turn の上限（`--max-turns <n>`）。`Some` の周だけ argv に載る——runner と lens は `None`
    /// （argv は不変）で、`fleet usage` の token refresh の起動だけが `Some(1)` を渡す（設計 fleet-usage.md §3）。
    pub max_turns: Option<u32>,
}

/// template の placeholder を **1 走査**で埋める。
///
/// `replace` を重ねると、**先に埋めた値の中に次の marker が在れば展開される**——契約は
/// 外から来る text なので、契約に `{write_set}` と書くだけで prompt の構造へ触れられて
/// しまう（実測 2026-09-10）。埋めた値を二度と走査しないことでその経路を塞ぐ。
pub fn fill(template: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    loop {
        let mut best: Option<(usize, &str, &str)> = None;
        for (key, value) in pairs {
            if let Some(at) = rest.find(key) {
                if best.is_none_or(|(found, _, _)| at < found) {
                    best = Some((at, key, value));
                }
            }
        }
        match best {
            None => {
                out.push_str(rest);
                return out;
            }
            Some((at, key, value)) => {
                out.push_str(rest.get(..at).unwrap_or_default());
                out.push_str(value);
                rest = rest.get(at.saturating_add(key.len())..).unwrap_or_default();
            }
        }
    }
}

/// [`Call`] から `Command` を組む。**prompt は argv でなく stdin で渡す**。
///
/// argv で渡すと Linux の 1 引数上限（`MAX_ARG_STRLEN` = 128KiB）に当たり、**user が
/// 裁定した cap 150000 が実質 130KB へ黙って切り下がる**（実測 2026-09-10: 131000 byte で
/// `Argument list too long`）。`claude -p` は prompt 引数が無ければ stdin から読む。
///
/// 包みの結果（unit 名）も返す——呼び手は子の終端で [`confine::release_scope`] を撃つ
/// （設計 gate-cost.md §4.4 errata・`s2-07l.234`）。
pub fn build(call: &Call<'_>) -> (Command, confine::Confinement) {
    let mut inner = Command::new(call.claude);
    inner
        .arg("-p")
        // permission mode は**毎回**渡す。省くと版の既定に従い、同じ 1 行が
        // 環境ごとに違う権限で走る。
        .arg("--permission-mode")
        .arg(call.permission_mode);
    // model も**毎回**渡す（`Some` の周・設計 pipeline.md §6）。permission mode と同じ理由で、省くと版の
    // 既定に従い、便が消費するモデル別窓と便用の口座選定が数える窓がずれる。置き場は呼出側でなくここ
    // （runner と lens の唯一の構築点）。
    if let Some(model) = call.model {
        inner.arg("--model").arg(model);
    }
    // effort も**毎回**渡す（`Some` の周・`--model` の直後）。省くと口座の設定 dir の settings が深さを決める。
    if let Some(effort) = call.effort {
        inner.arg("--effort").arg(effort);
    }
    inner
        // **settings を 1 つも読まない**（ADR-0011 §2.1）。空の値は user / project / local の
        // **どれも読まない**という意味で、`project` に絞る形では対象 repo の
        // `.claude/settings.json` の allow 規則が残る——便ごとに凍結した allowlist
        // （ADR-0010 §2.4）を、実装させている当の repo 側から広げられてしまう。
        //
        // **runner と lens の唯一の構築点がここ**である。片方の口だけで渡す形にすると、
        // もう片方が版の既定（= その周の口座と checkout の settings）で起きる。
        .arg("--setting-sources")
        .arg("")
        // MCP も同じ極性で閉じる。宣言していない server を拾わせない。
        .arg("--strict-mcp-config");
    if let Some(format) = call.output.flag() {
        inner.arg("--output-format").arg(format);
    }
    if call.output == Format::StreamJson {
        // `-p` と `stream-json` の併用は **この版の claude が `--verbose` を要求する**
        // （無いと `requires --verbose` で rc 1・実測 2026-09-10）。fake は flag を
        // 読まないので、これを落としても歯は緑のまま通る＝実 claude でだけ死ぬ。
        inner.arg("--verbose");
    }
    if let Some(turns) = call.max_turns {
        inner.arg("--max-turns").arg(turns.to_string());
    }
    // **`plugin_dir` は plugin の root**（設計 §6）: 配下の dir を名前順に 1 つずつ渡す。root を
    // そのまま渡して claude の folder 展開に任せる形にしないのは、読めない周・0 本の周の極性と
    // 読み込み順を器が握るためである（版依存に寄せない）。読めない・0 本の root は runner が
    // claude を起こす前に rc 2 で落とす（ここへ来ない）。
    if let Some(root) = call.plugin_dir {
        for dir in plugin_dirs(Path::new(root)).unwrap_or_default() {
            inner.arg("--plugin-dir").arg(dir);
        }
    }
    // **claude も cgroup の scope で包む**（設計 gate-cost.md §4.1 の 3 つ目）。包むのは argv が
    // 揃った後・cwd と env を付ける前である——`Command` からは cwd も env も stdio も読み戻せ
    // ないので、先に包まないと外側へ移せない。箱は 1 × `gate.job_memory_mb`（同 §12・裁定 id
    // user 2026-09-15T18:2xZ）で、包めない host では素のまま起きる（止めない）。
    let unit = confine::unit_name(claude_place(call), CLAUDE_STAGE, 1);
    let wrap = confine::Wrap {
        unit: &unit,
        limit: confine::Limit::PerJob(1),
        caps: confine::Caps::embedded(),
    };
    let (mut cmd, confinement) = confine::wrap_command(inner, &wrap);
    if let Some(dir) = call.cwd {
        cmd.current_dir(dir);
    }
    if let Some(dir) = call.account_dir {
        cmd.env(ACCOUNT_ENV, dir);
    }
    // **agent view は常に切る**（account-autonomy.md §5「agent view の前提」）: 有効な session は background work が残る周の
    // `/exit` で dialog を出して止まり、器は描画を読まない（C3.3）ので答えられない。runner と lens の唯一の構築点がここ
    // なので 1 か所で足りる（口座の有無に依らない・親の値は継承させず上書きする）。
    cmd.env(AGENT_VIEW_ENV, AGENT_VIEW_OFF);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
    (cmd, confinement)
}

/// plugin の root（`--plugin-dir` の値・設計 §6）の配下の **dir** を名前順に返す。
///
/// root 直下の file と symlink は plugin と見ない（`DirEntry::file_type` は link を辿らない＝
/// root の外を指す link 1 本で別の木を載せない）。**root が読めない・配下の dir が 0 の周は `Err`**
/// ——pipe の spawn は器の plugin を root へ必ず書くので、0 本は root が壊れた印である。
pub fn plugin_dirs(root: &Path) -> Result<Vec<PathBuf>, String> {
    let unreadable = |err: std::io::Error| format!("plugin root {} を読めない: {err}", root.display());
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(root).map_err(unreadable)? {
        let entry = entry.map_err(unreadable)?;
        if entry.file_type().map_err(unreadable)?.is_dir() {
            dirs.push(entry.path());
        }
    }
    if dirs.is_empty() {
        return Err(format!("plugin root {} の配下に plugin の dir が無い", root.display()));
    }
    dirs.sort();
    Ok(dirs)
}

/// 子の stdin へ prompt を書いて閉じる。
///
/// 読まずに終える子への write は EPIPE になるが、**判定は出力で決める**のでここの失敗は
/// 理由にしない（`take` で drop され、子は EOF を見る）。
pub fn feed(child: &mut std::process::Child, prompt: &str) {
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(prompt.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::{build, claude_place, plugin_dirs, Call, Format, FORMATS};
    use proptest::prelude::*;
    use proptest::test_runner::Config;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 反例の永続化を切り、case 数を pin する（`pipe/refuse.rs` と同じ形・case ごとに fs を触るので 64）。
    fn config() -> Config {
        Config {
            cases: 64,
            failure_persistence: None,
            ..Config::default()
        }
    }

    /// case の通番（tmp の root を case ごとに分ける）。
    static CASE: AtomicUsize = AtomicUsize::new(0);

    /// 名前集合の subdir（`d` 前置）と file（`f` 前置）と root 自身を指す symlink（`link`）を
    /// 置いた root を作り、期待する `--plugin-dir` の列（subdir の名前の昇順）と対で返す。
    fn layout(dirs: &BTreeSet<String>, files: &BTreeSet<String>) -> (PathBuf, Vec<String>) {
        let root = std::env::temp_dir().join(format!(
            "headless-plugin-root-{}-{}",
            std::process::id(),
            CASE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        for name in dirs {
            let _ = std::fs::create_dir(root.join(format!("d{name}")));
        }
        for name in files {
            let _ = std::fs::write(root.join(format!("f{name}")), "{}\n");
        }
        let _ = std::os::unix::fs::symlink(&root, root.join("link"));
        // BTreeSet の順＝名前の昇順（`d` 前置は順を変えない）。
        let want = dirs.iter().map(|name| root.join(format!("d{name}")).display().to_string()).collect();
        (root, want)
    }

    /// [`build`] の argv から `--plugin-dir` の値を順に集める（包みの有無に依らず argv の中を見る）。
    fn plugin_args(root: &Path) -> Vec<String> {
        let text = root.display().to_string();
        let (command, _) = build(&Call {
            claude: "claude",
            prompt: "",
            permission_mode: "plan",
            model: None,
            effort: None,
            plugin_dir: Some(&text),
            account_dir: None,
            cwd: None,
            output: Format::StreamJson,
            max_turns: None,
        });
        let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        args.windows(2)
            .filter(|pair| pair.first().is_some_and(|flag| flag == "--plugin-dir"))
            .filter_map(|pair| pair.get(1).cloned())
            .collect()
    }

    // flip-check: retroactive s2-07l.222
    /// `claude_place` は call の形ごとに scope の unit 名に載る口の名を返す: 逐次 record を受ける call は
    /// `runner`・受けない call は `lens`（空や別の字面に潰すと、runner と lens の scope が unit 名で弁別できない）。
    #[test]
    fn mutant_in_pipe_claude_place_names_runner_and_lens() {
        let call = |output: Format| Call {
            claude: "claude",
            prompt: "",
            permission_mode: "plan",
            model: None,
            effort: None,
            plugin_dir: None,
            account_dir: None,
            cwd: None,
            output,
            max_turns: None,
        };
        assert_eq!(claude_place(&call(Format::StreamJson)), "runner", "逐次 record を受ける口");
        assert_eq!(claude_place(&call(Format::Json)), "lens", "判定を 1 つ受け取る口");
        assert_eq!(claude_place(&call(Format::Text)), "lens", "計測の起動も lens の側（従来の名のまま）");
    }

    /// 出力形式の 3 値は argv に `--output-format` の値として写る: text は flag ごと渡さず（`fleet usage` の argv は
    /// 不変）・json は `json` の対だけ・stream-json は `stream-json` の対と `--verbose`（設計 gate-cost.md §26 形 (2)）。
    #[test]
    fn run_cost_output_format_is_in_argv_per_variant() {
        let args_of = |output: Format| {
            let (command, _) = build(&Call {
                claude: "claude",
                prompt: "",
                permission_mode: "plan",
                model: None,
                effort: None,
                plugin_dir: None,
                account_dir: None,
                cwd: None,
                output,
                max_turns: None,
            });
            command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<String>>()
        };
        assert_eq!(FORMATS.len(), 3, "閉じた 3 値");
        let text = args_of(Format::Text);
        assert!(!text.iter().any(|arg| arg == "--output-format" || arg == "--verbose"), "text は渡さない: {text:?}");
        let json = args_of(Format::Json);
        assert_eq!(json.windows(2).filter(|pair| pair == &["--output-format", "json"]).count(), 1, "{json:?}");
        assert!(!json.iter().any(|arg| arg == "--verbose"), "json は --verbose を要らない: {json:?}");
        assert_eq!(json.len(), text.len() + 2, "足されるのは対の 2 引数だけ: {json:?} / {text:?}");
        let stream = args_of(Format::StreamJson);
        assert_eq!(stream.windows(2).filter(|pair| pair == &["--output-format", "stream-json"]).count(), 1, "{stream:?}");
        assert_eq!(stream.len(), text.len() + 3, "対と --verbose の 3 引数: {stream:?}");
    }

    /// `max_turns: Some(1)` の call は argv に `--max-turns` `1` が隣り合って並び、`None` の call には
    /// `--max-turns` が 1 本も現れない（runner / lens の argv は不変）。
    #[test]
    fn headless_call_max_turns_is_in_argv_only_when_some() {
        let args_of = |max_turns: Option<u32>| {
            let (command, _) = build(&Call {
                claude: "claude",
                prompt: "",
                permission_mode: "plan",
                model: None,
                effort: None,
                plugin_dir: None,
                account_dir: None,
                cwd: None,
                output: Format::Text,
                max_turns,
            });
            command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<String>>()
        };
        let some = args_of(Some(1));
        assert_eq!(
            some.windows(2).filter(|pair| pair == &["--max-turns", "1"]).count(),
            1,
            "Some(1) は --max-turns 1 の対が 1 つ: {some:?}"
        );
        let none = args_of(None);
        assert!(!none.iter().any(|arg| arg == "--max-turns"), "None では現れない: {none:?}");
        assert_eq!(some.len(), none.len() + 2, "足されるのは対の 2 引数だけ: {some:?} / {none:?}");
    }

    /// `model: Some("opus")` の call は argv に `--model` `opus` が隣り合って並び、`None` の call には `--model` が
    /// 1 本も現れない（`fleet usage` の refresh の argv は不変・`s2-07l.297`）。置き場は `--permission-mode` の対の直後。
    #[test]
    fn headless_call_model_is_in_argv_only_when_some() {
        let args_of = |model: Option<&str>| {
            let (command, _) = build(&Call {
                claude: "claude",
                prompt: "",
                permission_mode: "plan",
                model,
                effort: None,
                plugin_dir: None,
                account_dir: None,
                cwd: None,
                output: Format::Text,
                max_turns: None,
            });
            command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<String>>()
        };
        let some = args_of(Some("opus"));
        assert_eq!(
            some.windows(2).filter(|pair| pair == &["--model", "opus"]).count(),
            1,
            "Some は --model opus の対が 1 つ: {some:?}"
        );
        let at_mode = some.iter().position(|arg| arg == "--permission-mode");
        let at_model = some.iter().position(|arg| arg == "--model");
        assert_eq!(at_model, at_mode.map(|at| at + 2), "permission mode の対の直後: {some:?}");
        let none = args_of(None);
        assert!(!none.iter().any(|arg| arg == "--model"), "None では現れない: {none:?}");
        assert_eq!(some.len(), none.len() + 2, "足されるのは対の 2 引数だけ: {some:?} / {none:?}");
    }

    /// 名前の集合（大小文字を混ぜて byte 順が自明でない形・0〜5 個）。
    fn names() -> impl Strategy<Value = BTreeSet<String>> {
        prop::collection::btree_set("[A-Za-z0-9_-]{1,6}", 0..6)
    }

    proptest! {
        #![proptest_config(config())]

        /// `--plugin-dir` の列は subdir の名前の昇順 ∧ 件数一致 ∧ file と symlink を含まない
        /// （期待列は subdir だけで組むので、等号がそのまま「file を含まない」を測る）。
        /// dir 0 の root は `Err`（runner が rc 2 で落とす極性）で、argv にも 1 本も載らない。
        #[test]
        fn prop_plugin_root_expands_subdirs_in_name_order(dirs in names(), files in names()) {
            let (root, want) = layout(&dirs, &files);
            let got = plugin_args(&root);
            let checked = plugin_dirs(&root);
            let _ = std::fs::remove_dir_all(&root);
            prop_assert_eq!(&got, &want);
            prop_assert_eq!(got.len(), dirs.len());
            prop_assert_eq!(checked.is_err(), dirs.is_empty());
        }
    }
}

