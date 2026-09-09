//! `<NAME> lens`（設計 §6・FR9 / NFR1）。diff を stdin で受け、判定の JSON 1 行を返す。
//!
//! **cap を超えた diff では claude を呼ばない**。呼んでから「長すぎた」と言うのでは、
//! 上限を置いた意味（NFR1）が無い。判定に届かなかった周はすべて INCONCLUSIVE へ倒す——
//! 偽の PASS を作らないためである（AC3）。

use super::{build, flag, need, read_stdin_bytes, Call, DEFAULT_CLAUDE};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};

/// prompt の文面（tracked な template・絶対 path も口座名も含まない）。
const TEMPLATE: &str = include_str!("lens.txt");

/// JSON 行の見出し。
const JSON_HEAD: char = '{';

/// 使い方の 1 行。
pub fn usage() -> String {
    format!(
        "usage: {} lens --cap BYTES --permission-mode M [--account-dir D] [--claude PATH] < diff",
        crate::name::NAME
    )
}

/// 判定に届かなかった 1 行を組む。
fn inconclusive(reason: &str) -> String {
    format!(r#"{{"verdict":"INCONCLUSIVE","evidence":"{reason}"}}"#)
}

/// `lens` を 1 回。diff は stdin から byte で読む。
pub fn dispatch(args: &[String]) -> Outcome {
    let parsed = (|| {
        Ok::<_, String>((
            need(args, "--cap")?.to_owned(),
            need(args, "--permission-mode")?.to_owned(),
            flag(args, "--account-dir")?.map(str::to_owned),
            flag(args, "--claude")?.map(str::to_owned),
        ))
    })();
    let (cap, mode, account, claude) = match parsed {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("lens: {reason}"), usage()]),
    };
    let Ok(cap) = cap.parse::<usize>() else {
        return Outcome::failed_line(RC_REFUSED, format!("lens: --cap は byte 数である（{cap}）"));
    };
    let diff = read_stdin_bytes();
    if diff.len() > cap {
        // **claude を呼ばずに**返す。呼ばないことが cap の意味である。
        return Outcome::ok_line(inconclusive("diff exceeds cap"));
    }
    let prompt = TEMPLATE.replace("{diff}", &String::from_utf8_lossy(&diff));
    ask(&Call {
        claude: claude.as_deref().unwrap_or(DEFAULT_CLAUDE),
        prompt: &prompt,
        permission_mode: &mode,
        plugin_dir: None,
        account_dir: account.as_deref(),
        cwd: None,
        // 判定を 1 つ受け取るだけなので既定（text）で呼ぶ。stream-json にすると
        // 「最後の JSON 行」が claude の result record になり、判定が取れない。
        streaming: false,
    })
}

/// claude を呼び、出力の**最後の JSON 行**を stdout 1 行に写す。
fn ask(call: &Call<'_>) -> Outcome {
    let spawned = build(call).output();
    let out = match spawned {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("lens: claude を起動できない: {err}")),
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let found = text
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with(JSON_HEAD));
    match found {
        // 読めない出力を握り潰さない。**判定に届かなかった**と名乗る。
        None => Outcome::ok_line(inconclusive("lens output has no json line")),
        Some(line) => Outcome::ok_line(line.trim().to_owned()),
    }
}
