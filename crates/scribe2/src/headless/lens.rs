//! `<NAME> lens`（設計 §6・FR9 / NFR1）。契約を `--contract` の path で・diff を stdin で
//! 受け、判定の JSON 1 行を返す。
//!
//! **契約が無ければ claude を呼ばない**。lens に問うのは「diff が契約の求めるものを
//! 満たすか」であって、diff だけを渡して同じことを問うことはできない——実 lens は
//! 「契約が未提供で適合を判定できない」と正しく INCONCLUSIVE を返し、便はそこで止まる
//! （実測 2026-09-10・s2-07l.24 の実 5 便）。**材料の不足は前提違反として断る**（rc 1）。
//! 契約を prompt へ差し込んでから判定を問うのが lens の口である。
//!
//! **cap を超えた diff では claude を呼ばない**。呼んでから「長すぎた」と言うのでは、
//! 上限を置いた意味（NFR1）が無い。判定に届かなかった周はすべて INCONCLUSIVE へ倒す——
//! 偽の PASS を作らないためである（AC3）。

use super::{build, feed, fill, flag, need, read_stdin_bytes, Call, DEFAULT_CLAUDE};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::pipe::contract::Contract;
use std::path::Path;

/// prompt の文面（tracked な template・絶対 path も口座名も含まない）。
const TEMPLATE: &str = include_str!("lens.txt");

/// JSON 行の見出し。
const JSON_HEAD: char = '{';

/// 使い方の 1 行。
pub fn usage() -> String {
    format!(
        "usage: {} lens --contract F --cap BYTES --permission-mode M [--account-dir D] [--claude PATH] < diff",
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
            need(args, "--contract")?.to_owned(),
            need(args, "--cap")?.to_owned(),
            need(args, "--permission-mode")?.to_owned(),
            flag(args, "--account-dir")?.map(str::to_owned),
            flag(args, "--claude")?.map(str::to_owned),
        ))
    })();
    let (contract, cap, mode, account, claude) = match parsed {
        Ok(found) => found,
        Err(reason) => return Outcome::failed(RC_REFUSED, vec![format!("lens: {reason}"), usage()]),
    };
    // **読めない契約で claude を起こさない**。材料が無いまま問えば返るのは
    // INCONCLUSIVE だけで、払った 1 回分は捨て金になる。
    let contract = match Contract::load(Path::new(&contract)) {
        Ok(found) => found,
        Err(errors) => {
            let first = errors.first().map_or_else(String::new, |err| err.reason.clone());
            return Outcome::failed_line(RC_BROKEN, format!("lens: 契約を読めない: {first}"));
        }
    };
    let Ok(cap) = cap.parse::<usize>() else {
        return Outcome::failed_line(RC_REFUSED, format!("lens: --cap は byte 数である（{cap}）"));
    };
    let diff = read_stdin_bytes();
    if diff.len() > cap {
        // **claude を呼ばずに**返す。呼ばないことが cap の意味である。
        return Outcome::ok_line(inconclusive("diff exceeds cap"));
    }
    // **1 走査で埋める**。重ねて replace すると、先に埋めた契約本文の中の `{diff}` まで
    // 展開され、外から来る text が prompt の構造へ触れられる（runner と同じ理由）。
    let stated = state(&contract);
    let prompt = fill(
        TEMPLATE,
        &[("{contract}", &stated), ("{diff}", &String::from_utf8_lossy(&diff))],
    );
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

/// 契約を prompt へ差し込む形に組む（goal / done / verify 各行 / write-set 各行）。
///
/// **契約 file を丸写ししない**。lens が要るのは「何を作る契約か」と「何で測るか」で、
/// owner や disposition は判定の材料にならない——渡すほど cap（NFR1）を食う。
fn state(contract: &Contract) -> String {
    let listed = |lines: &[String]| {
        lines
            .iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "goal: {}\ndone: {}\nverify:\n{}\nwrite-set:\n{}",
        contract.goal,
        contract.done,
        listed(&contract.verify),
        listed(&contract.write_set),
    )
}

/// claude を呼び、出力の**最後の JSON 行**を stdout 1 行に写す。
fn ask(call: &Call<'_>) -> Outcome {
    let spawned = build(call).spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("lens: claude を起動できない: {err}")),
    };
    feed(&mut child, call.prompt);
    let out = match child.wait_with_output() {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("lens: claude の出力を読めない: {err}")),
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
