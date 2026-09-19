//! `pipe preflight` — 受付と同じ判定を run を作らず撃ち、契約の実態突合を planner が edit time に測る口（契約表の行 u・
//! 設計 contract-source.md §21・SRS FR48・C16「逸脱は edit time に止める」）。
//!
//! 引数は `intake` と同じ（`--design <doc>#<id> --bead B --repo R [--state-dir S]`）。判定は受付の [`super::intake::judge`] の
//! **同じ 1 本**（C2・2 本目を作らない）で、断りを最初の 1 件で止めず**全部**（判定関数 1 本につき高々 1 件）並べる。
//! run dir・写し・event は一切書かず、宣言の写しは読むだけ・置き場は交差の読みにだけ使う。
//!
//! stdout は **1 行 1 事実**: `design=<doc>#<id> section=<n>` / `write-set=<declared|derived> files=<n>` /
//! `teeth=<filter>:<本数>@<file,…>`（verify の nextest 行ごと）/ `headroom=<file>:<余地>/<size の見積>`（余地の小さい順）/
//! `overlap=<live run>:<file,…>`（突き合わせた live な run ごと・交差 0 は `-`・置き場が無ければ `overlap=unmeasured`）/
//! `refuse=<名>:<理由>`（judge の断り・全部・名は [`crate::pipe::refuse::Refuse::as_str`]）/ 末尾に
//! `preflight: <ok|refused n=<件数>|broken>`。rc = 0（断り 0）/ 1（断り ≥ 1）/ 2（読めない = `RC_BROKEN` の周）。
//! `--state-dir` が無く git 設定からも解けない周は `overlap=unmeasured` を出し、rc は他の断りで決める（測れないを 0 に
//! 潰さない・C10・`intake` は従来どおり置き場が無い旨で断る）。

use super::intake::{ceiling_of, generated, judge, read_args, Denial, Judged, Material, Materials};
use super::state_dir_of;
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_OK, RC_REFUSED};
use crate::rules::manifest::Manifest;

/// 末尾の判定行の書き出し。
const TAIL: &str = "preflight:";

/// 置き場が無く交差を測れなかった周の行。
const UNMEASURED: &str = "overlap=unmeasured";

/// 空の file の列の字面（交差 0・歯の file 0）。
const NONE: &str = "-";

/// `pipe preflight`: judge だけを撃ち、事実と断りを stdout に並べる。
pub(super) fn preflight(args: &[String], manifest: &Manifest) -> Outcome {
    let (pointer, bead, repo) = match read_args(args) {
        Ok(found) => found,
        Err(denial) => return denial.outcome,
    };
    let ceiling = match ceiling_of(manifest) {
        Ok(found) => found,
        Err(denial) => return denial.outcome,
    };
    // 受付と**同じ 1 本**で base の行から契約を組む（C2）。行を引けない・表の検査に落ちる周は判定の対象が
    // 揃わないので、受付と同じ断りをそのまま返して末尾に `broken` を積む（0 件と混ぜない）。
    // **repo の材料の読みは 1 回**（設計 dispatcher.md §5）。生成も判定も同じ 1 つを借りる。
    let materials = match Materials::read(&repo, &ceiling.borrow()) {
        Ok(found) => found,
        Err(denial) => return denial.outcome,
    };
    let contract = match generated(&repo, &pointer, &materials) {
        Ok((found, _)) => found,
        Err(denial) => {
            // 末尾は **rc に従う**（読めない = broken・撃てない = 断り 1 件）。行の欠陥は「読めない」ではない。
            let mut outcome = denial.outcome;
            let tail = if outcome.rc == RC_BROKEN { format!("{TAIL} broken") } else { format!("{TAIL} refused n=1") };
            outcome.out.push(tail);
            return outcome;
        }
    };
    // 置き場は交差と重複 run の 2 検査にだけ要る。解けない周は断りでなく `overlap=unmeasured`。
    let state_dir = state_dir_of(args).ok();
    let material = Material {
        repo: &repo,
        manifest,
        contract: &contract,
        state_dir: state_dir.as_deref(),
        bead: &bead,
        materials: &materials,
    };
    render(&judge(&material), state_dir.is_some())
}

/// judge の結果を 1 行 1 事実に描く。`measured` は置き場が在った（交差を撃った）か。
fn render(judged: &Judged, measured: bool) -> Outcome {
    let mut out: Vec<String> = Vec::new();
    if let Some((design, section)) = &judged.design {
        out.push(format!("design={design} section={section}"));
    }
    if let Some((kind, files)) = judged.write_set {
        out.push(format!("write-set={} files={files}", kind.as_str()));
    }
    for (filter, files) in &judged.teeth {
        out.push(format!("teeth={filter}:{}@{}", files.len(), listed(files)));
    }
    if let Some(found) = &judged.headroom {
        out.extend(found.rooms.iter().map(|(file, room)| format!("headroom={file}:{room}/{}", found.size_lines)));
    }
    if !measured {
        out.push(UNMEASURED.to_owned());
    }
    if let Some(found) = &judged.overlap {
        out.extend(found.runs.iter().map(|(run, files)| format!("overlap={run}:{}", listed(files))));
    }
    out.extend(judged.denials.iter().map(refuse_line));
    let broken = judged.denials.iter().any(|denial| denial.outcome.rc == RC_BROKEN);
    let (rc, tail) = match (judged.denials.len(), broken) {
        (0, _) => (RC_OK, "ok".to_owned()),
        (_, true) => (RC_BROKEN, "broken".to_owned()),
        (count, false) => (RC_REFUSED, format!("refused n={count}")),
    };
    out.push(format!("{TAIL} {tail}"));
    Outcome { out, err: Vec::new(), rc }
}

/// `refuse=<名>:<理由>` の 1 行（stderr の行の `pipe: ` を剥がし、理由の後ろに並ぶ行〔交差の全組・余地の残り〕は
/// ` / ` で同じ行に続ける＝1 断り 1 行）。
fn refuse_line(denial: &Denial) -> String {
    let reasons: Vec<&str> = denial.outcome.err.iter().map(|line| line.strip_prefix("pipe: ").unwrap_or(line)).collect();
    format!("refuse={}:{}", denial.name, reasons.join(" / "))
}

/// file の列を 1 つの語に（空は [`NONE`]）。
fn listed(files: &[String]) -> String {
    if files.is_empty() {
        NONE.to_owned()
    } else {
        files.join(",")
    }
}
