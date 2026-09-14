//! `<NAME> lens`（設計 §6・FR9 / NFR1）。契約を `--contract` の path で・diff を stdin で
//! 受け、判定の JSON 1 行を返す。
//!
//! **契約が無ければ claude を呼ばない**。lens に問うのは「diff が契約の求めるものを
//! 満たすか」であって、diff だけを渡して同じことを問うことはできない——実 lens は
//! 「契約が未提供で適合を判定できない」と正しく INCONCLUSIVE を返し、便はそこで止まる
//! （実測 2026-09-10・s2-07l.24 の実 5 便）。**材料の不足は前提違反として断る**（rc 1）。
//! 契約を prompt へ差し込んでから判定を問うのが lens の口である。
//!
//! **`--worktree` は必須である**。lens に憲法（生成区間を持つ `CLAUDE.md`）を載せる経路は
//! 起動 cwd 1 本なので、渡されなければ claude を起こさず rc 1 で断る——継承した cwd に
//! 頼ると、呼び手が変わった周に憲法の載らない判定が静かに出る（fail-closed・C11.2）。
//!
//! **cap を超えた diff では claude を呼ばない**。呼んでから「長すぎた」と言うのでは、
//! 上限を置いた意味（NFR1）が無い。判定に届かなかった周はすべて INCONCLUSIVE へ倒す——
//! 偽の PASS を作らないためである（AC3）。
//!
//! **cap の値は rules 行 `gate.token_cap` からだけ読む**（憲法 C1・`s2-07l.272`）。以前は
//! argv の `--cap` で受けていたので、値の出所が manifest（gate の判定）と launcher の手書き
//! （lens の判定）の 2 つに割れ、rules 行を上げても lens 側は旧値のまま INCONCLUSIVE を
//! 返した（実測 2026-09-14・`.265` / `.267`）。`--rules PATH` が在ればその manifest・無ければ
//! 埋め込み（`pipe::cli` と同じ規約）。`--cap` は**未知の引数として断る**（黙って読み飛ばすと、
//! 手書きの数が残った launcher が「効いている」ように見える）。

use super::{build, feed, fill, flag, need, read_stdin_bytes, Call, DEFAULT_CLAUDE};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::pipe::confine;
use crate::pipe::contract::Contract;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use std::path::Path;

/// prompt の文面（tracked な template・絶対 path も口座名も含まない）。
const TEMPLATE: &str = include_str!("lens.txt");

/// JSON 行の見出し。
const JSON_HEAD: char = '{';

/// diff の byte 数の上限を持つ rules 行（gate の判定と同じ 1 行・`pipe::cli::ROW_CAP` と同じ id）。
const ROW_CAP: &str = "gate.token_cap";

/// lens が受ける flag の全部（この外は未知の引数として断る）。
const KNOWN_FLAGS: [&str; 6] = ["--contract", "--worktree", "--permission-mode", "--rules", "--account-dir", "--claude"];

/// 使い方の 1 行。
pub fn usage() -> String {
    format!(
        "usage: {} lens --contract F --worktree D --permission-mode M [--rules PATH] [--account-dir D] [--claude PATH] < diff",
        crate::name::NAME
    )
}

/// 判定に届かなかった 1 行を組む。
fn inconclusive(reason: &str) -> String {
    format!(r#"{{"verdict":"INCONCLUSIVE","evidence":"{reason}"}}"#)
}

/// [`KNOWN_FLAGS`] の外の引数を 1 つ名指す（無ければ `None`）。
///
/// flag の次の 1 語は値として飛ばす（`--` で始まる語は値でなく次の flag として読む＝値欠けは
/// [`flag`] が「値が無い」で断る）。撤去した `--cap` をここで捕まえる＝launcher の手書きは構造で止まる。
fn unknown_arg(args: &[String]) -> Option<&str> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        if !KNOWN_FLAGS.contains(&arg.as_str()) {
            return Some(arg);
        }
        at += 1;
        if args.get(at).is_some_and(|value| !value.starts_with("--")) {
            at += 1;
        }
    }
    None
}

/// cap（byte）を rules 行から読む。`--rules PATH` が在ればその manifest・無ければ埋め込み。
///
/// 行が無い / 不発効 / 整数でない周は `pipe::cli::int_row` と同じ 3 理由で `Err`（同形の関数を
/// 2 つ持つのは、`pipe` 側が private で、pub にするには write-set の外へ手を入れるからである）。
fn cap_of(args: &[String]) -> Result<u64, String> {
    let loaded = match flag(args, "--rules")? {
        Some(path) => Manifest::load(Path::new(path)),
        None => Manifest::embedded(),
    };
    let manifest = loaded.map_err(|errors| {
        let joined = errors.iter().map(ToString::to_string).collect::<Vec<String>>().join(" / ");
        format!("rules を読めない: {joined}")
    })?;
    let row = manifest.get(ROW_CAP).ok_or(format!("{ROW_CAP} が無い"))?;
    if !row.enabled {
        return Err(format!("{ROW_CAP} は不発効である"));
    }
    match row.value {
        RuleValue::Int(found) => Ok(found),
        _ => Err(format!("{ROW_CAP} が整数でない")),
    }
}

/// `lens` を 1 回。diff は stdin から byte で読む。
pub fn dispatch(args: &[String]) -> Outcome {
    let parsed = (|| {
        if let Some(found) = unknown_arg(args) {
            return Err(format!("未知の引数 {found}"));
        }
        Ok::<_, String>((
            need(args, "--contract")?.to_owned(),
            need(args, "--worktree")?.to_owned(),
            need(args, "--permission-mode")?.to_owned(),
            flag(args, "--account-dir")?.map(str::to_owned),
            flag(args, "--claude")?.map(str::to_owned),
        ))
    })();
    let (contract, worktree, mode, account, claude) = match parsed {
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
    // **cap が解けない周も claude を起こさない**（上限なしで走らせない＝C6）。
    let cap = match cap_of(args) {
        Ok(found) => found,
        Err(reason) => return Outcome::failed_line(RC_BROKEN, format!("lens: {reason}")),
    };
    let diff = read_stdin_bytes();
    if u64::try_from(diff.len()).unwrap_or(u64::MAX) > cap {
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
        // **便の worktree で起こす**（anchor の repo は渡さない）。判定に載る憲法は
        // base の checkout のものであり、anchor 側の未 commit な `CLAUDE.md` ではない。
        cwd: Some(Path::new(&worktree)),
        // 判定を 1 つ受け取るだけなので既定（text）で呼ぶ。stream-json にすると
        // 「最後の JSON 行」が claude の result record になり、判定が取れない。
        streaming: false,
        max_turns: None,
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
///
/// 起動形は [`build`] が持つ——lens は `--allowedTools` を渡さない側だが、settings 由来の
/// allow 規則は権限の口を開けるので、settings を 1 つも読まない形（`--setting-sources` の
/// 空値 + `--strict-mcp-config`）は runner と同じく毎回効く（ADR-0011 §2.1）。
fn ask(call: &Call<'_>) -> Outcome {
    let (mut command, confinement) = build(call);
    let spawned = command.spawn();
    let mut child = match spawned {
        Ok(found) => found,
        Err(err) => return Outcome::failed_line(RC_BROKEN, format!("lens: claude を起動できない: {err}")),
    };
    feed(&mut child, call.prompt);
    let waited = child.wait_with_output();
    // **終端で scope を片付ける**（設計 gate-cost.md §4.4 errata・`s2-07l.234`）。stdout の 1 行は
    // 判定の面なので、結果は stderr の 1 行だけに出す。
    let mut outcome = read_verdict(waited);
    let scope = confine::release_scope(&confinement);
    outcome.err.extend(scope.map(|released| format!("lens: scope={}", released.as_str())));
    outcome
}

/// 終わった claude の出力から最後の JSON 行を読む。
fn read_verdict(waited: std::io::Result<std::process::Output>) -> Outcome {
    let out = match waited {
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
