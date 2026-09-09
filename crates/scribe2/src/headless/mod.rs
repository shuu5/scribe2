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

pub mod lens;
pub mod runner;

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

/// 既定の claude 実行 file。`--claude` は **test の seam**（fake の実行 file を渡す）。
pub const DEFAULT_CLAUDE: &str = "claude";

/// rate limit で止めた周の rc。呼出側（`pipe`）はこれを `Failed detail=rate-limit` に写す。
pub const RC_RATE_LIMIT: u8 = 75;

/// 口座の切替に使う**子 process の**環境変数。ここへ書くだけで、自分では読まない。
pub const ACCOUNT_ENV: &str = "CLAUDE_CONFIG_DIR";

/// 判定に届かなかった周の 1 行（lens の既定）。
pub const INCONCLUSIVE_HEAD: &str = r#"{"verdict":"INCONCLUSIVE","evidence":"#;

/// flag の値を取る。値が無ければ理由つきで `Err`。
///
/// `pipe::cli` に同形の関数が在るが、そちらは private であり、pub にするには
/// `pipe` 側へ手を入れることになる（本契約の「やらない」に当たる）。**同じ形を
/// 2 つ持つより、契約の柵を守るほうを採った**。
pub fn flag<'a>(args: &'a [String], name: &str) -> Result<Option<&'a str>, String> {
    let Some(at) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    match args.get(at + 1) {
        Some(found) if !found.starts_with("--") => Ok(Some(found)),
        _ => Err(format!("{name} に値が無い")),
    }
}

/// 必須の flag。
pub fn need<'a>(args: &'a [String], name: &str) -> Result<&'a str, String> {
    flag(args, name)?.ok_or(format!("{name} が要る"))
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
    /// 本 repo の plugin を載せる dir。
    pub plugin_dir: Option<&'a str>,
    /// 口座の設定 dir（子の環境変数へ書く値）。
    pub account_dir: Option<&'a str>,
    /// 起動する cwd。
    pub cwd: Option<&'a Path>,
    /// record を**逐次**受け取るか（`--output-format stream-json`）。
    ///
    /// 逐次が要るのは runner だけである——rate limit の record を**途中で**見て止める
    /// ためで、lens は判定を 1 つ受け取るだけなので既定（text）で呼ぶ。stream-json は
    /// **全行が JSON** ゆえ「最後の JSON 行」が claude 自身の result record になり、
    /// モデルの判定は record の中の文字列へ埋もれる（実測 2026-09-10）。
    pub streaming: bool,
}

/// [`Call`] から `Command` を組む。**stdout だけ piped**（stderr は素通し）。
pub fn build(call: &Call<'_>) -> Command {
    let mut cmd = Command::new(call.claude);
    cmd.arg("-p")
        .arg(call.prompt)
        // permission mode は**毎回**渡す。省くと版の既定に従い、同じ 1 行が
        // 環境ごとに違う権限で走る。
        .arg("--permission-mode")
        .arg(call.permission_mode);
    if call.streaming {
        // `-p` と `stream-json` の併用は **この版の claude が `--verbose` を要求する**
        // （無いと `requires --verbose` で rc 1・実測 2026-09-10）。fake は flag を
        // 読まないので、これを落としても歯は緑のまま通る＝実 claude でだけ死ぬ。
        cmd.arg("--output-format").arg("stream-json").arg("--verbose");
    }
    if let Some(dir) = call.plugin_dir {
        cmd.arg("--plugin-dir").arg(dir);
    }
    if let Some(dir) = call.cwd {
        cmd.current_dir(dir);
    }
    if let Some(dir) = call.account_dir {
        cmd.env(ACCOUNT_ENV, dir);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped());
    cmd
}
