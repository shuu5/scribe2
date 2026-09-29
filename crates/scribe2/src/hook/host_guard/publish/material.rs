//! publish の走査の段（設計 docs/design/vessel-hook.md §22 行 n5・ADR-0078 / ADR-0093・SRS FR80 / AC50 / AC63）。
//!
//! 隣が在る segment ごとに、隣（名札 = anchor の basename・名の列 = basename と導けた name）・公開先（対象の name と PUBLIC の anchor の
//! name）・除外（host の面の字句）・行の要素・出ていく字面を照合の核（[`scan`]）へ 1 回渡し、越えの印の在る segment は照合せず
//! oversize、当たりは identifier で断る。object id・tracked path・台帳 id の材料は後続の行（行 n6）が足す。

use super::outgoing::{Denial, Found};
use super::probe::READ_ROW;
use super::scan::{scan, Neighbor, Phrases, Public};
use super::visibility::{Anchor, Sighted};
use super::{elements, Reason};
use crate::hook::host_guard::PUBLISH_ROW;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;

/// 重複を除いて足す（出現の順を保つ）。
fn add(names: &mut Vec<String>, name: &str) {
    if !names.iter().any(|found| found == name) {
        names.push(name.to_owned());
    }
}

/// owner/name の name。
fn name_of(repo: &str) -> Option<&str> {
    repo.split_once('/').map(|(_, name)| name)
}

/// 隣ごとの [`Neighbor`]（名札は dir の basename・名の列は basename と導けた name〔重複なし〕・object / path / 台帳の欄は空）。
fn neighbors(sighted: &Sighted) -> Vec<Neighbor> {
    let one = |anchor: &Anchor| {
        let mut names = vec![anchor.label.clone()];
        if let Some(name) = anchor.repo.as_deref().and_then(name_of) {
            add(&mut names, name);
        }
        Neighbor { tag: anchor.label.clone(), names, ..Neighbor::default() }
    };
    sighted.neighbors.iter().map(one).collect()
}

/// 公開先の [`Public`]（名の列は対象の name と PUBLIC の anchor の name〔重複なし〕）。
fn public(found: &Found) -> Public {
    let mut names = Vec::new();
    for name in found.own.iter().flatten().filter_map(|repo| name_of(repo)) {
        add(&mut names, name);
    }
    for name in &found.sighted.public {
        add(&mut names, name);
    }
    Public { names, ..Public::default() }
}

/// 走査の段（**1 関数**・段の入口が可視性の後に呼ぶ）: 隣が在る segment を順に、越えの印が立っていれば照合せず oversize、そうでなければ
/// [`scan`] を 1 回呼び当たりを identifier で断る。最初の断りで止まる。`ruling` は publish の行の裁定 id。
pub(super) fn check(found: &[Found], manifest: &Manifest, host: &Manifest, ruling: &str) -> Option<Denial> {
    let forms = match manifest.get(PUBLISH_ROW).map(|row| &row.value) {
        Some(RuleValue::List(values)) => elements(values).unwrap_or_default(),
        _ => Default::default(),
    };
    let phrases = Phrases { phrases: host.publish_exclusions().iter().map(|found| found.phrase().to_owned()).collect() };
    for seg in found.iter().filter(|seg| !seg.sighted.neighbors.is_empty()) {
        if seg.texts.over {
            let ruling = manifest.get(READ_ROW).map_or_else(|| "-".to_owned(), |row| row.ruling.clone());
            return Some(Denial { reason: Reason::Oversize, word: READ_ROW.to_owned(), row: READ_ROW, ruling, route: None });
        }
        let hits = scan(&forms, &seg.texts.texts(), &neighbors(&seg.sighted), &public(seg), &phrases);
        if let Some(word) = hits {
            return Some(Denial { reason: Reason::Identifier, word, row: PUBLISH_ROW, ruling: ruling.to_owned(), route: None });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{check, neighbors, public};
    use crate::hook::host_guard::publish::outgoing::Found;
    use crate::hook::host_guard::publish::scan::Source;
    use crate::hook::host_guard::publish::texts::{Body, Texts};
    use crate::hook::host_guard::publish::visibility::{Anchor, Sighted};
    use crate::hook::host_guard::publish::Reason;
    use crate::rules::manifest::Manifest;

    /// 公開の行（form repo-name）と上限の 2 行を持つ manifest（裁定 id は `r-<行 id>`）。
    fn manifest() -> Manifest {
        let row = |id: &str, kind: &str, value: &str| format!("\n[[rule]]\nid = \"{id}\"\nkind = \"{kind}\"\nvalue = {value}\nenabled = true\nruling = \"r-{id}\"\nruled_at = \"d\"\n");
        let text = format!(
            "schema = 1\n{}{}{}",
            row("host_guard.publish", "HostGuardPublish", "[\"form repo-name\"]"),
            row("host_guard.publish_deadline_ms", "HostGuardPublishDeadlineMs", "6000"),
            row("host_guard.publish_read_bytes", "HostGuardPublishReadBytes", "8388608"),
        );
        Manifest::parse(&text).unwrap_or_else(|errors| panic!("fixture の manifest を読める: {errors:?}"))
    }

    /// 隣 2 つ（導けた anchor と導けなかった anchor）・PUBLIC の anchor の name `open` を持つ segment（対象は acme/pub・本文は 1 つ）。
    fn found(body: &str, over: bool) -> Found {
        let neighbor = |label: &str, repo: Option<&str>| Anchor { label: label.to_owned(), repo: repo.map(str::to_owned) };
        let sighted = Sighted { private: false, neighbors: vec![neighbor("inner", Some("acme/secret")), neighbor("lost", None)], public: vec!["open".to_owned()] };
        let texts = Texts { bodies: vec![Body { source: Source::CommitMessage, body: body.to_owned() }], over };
        Found { texts, sighted, own: vec![Some("acme/pub".to_owned())] }
    }

    /// 行 n5 (a) 隣が PUBLIC でない anchor で名札は basename・名の列は basename と導けた name（重複なし）、既に公開の名は対象の name と PUBLIC の
    /// anchor の name: 隣の名は identifier（件数と先頭・row は publish の行・裁定 id は渡した id）で断り、既に公開の名は通し、隣が 0 の segment は通す。
    #[test]
    fn publish_scan_names_the_neighbors_by_basename_and_spares_the_public_names() {
        let seg = found("", false);
        let got: Vec<(String, Vec<String>)> = neighbors(&seg.sighted).into_iter().map(|one| (one.tag, one.names)).collect();
        let want = [("inner".to_owned(), vec!["inner".to_owned(), "secret".to_owned()]), ("lost".to_owned(), vec!["lost".to_owned()])];
        assert_eq!(got, want);
        let same = Sighted { neighbors: vec![Anchor { label: "same".to_owned(), repo: Some("acme/same".to_owned()) }], ..Sighted::default() };
        assert_eq!(neighbors(&same).first().map(|one| one.names.clone()), Some(vec!["same".to_owned()]), "重複なし");
        assert_eq!(public(&seg).names, ["pub", "open"]);
        assert!(neighbors(&seg.sighted).iter().all(|one| one.objects.is_empty() && one.paths.is_empty() && one.ledger.is_empty()));
        let (rules, face) = (manifest(), Manifest::default());
        let denied = check(&[found("fix secret and pub and open", false)], &rules, &face, "ruling-x").unwrap_or_else(|| panic!("隣の名は断る"));
        assert_eq!((denied.reason, denied.word.as_str(), denied.row, denied.ruling.as_str()), (Reason::Identifier, "1:repo-name=secret@inner", "host_guard.publish", "ruling-x"));
        let two = check(&[found("inner lost", false)], &rules, &face, "r").map(|one| one.word);
        assert_eq!(two.as_deref(), Some("2:repo-name=inner@inner,repo-name=lost@lost"));
        assert!(check(&[found("pub open", false)], &rules, &face, "r").is_none(), "既に公開の名は通す");
        let alone = Found { sighted: Sighted::default(), ..found("secret", false) };
        assert!(check(&[alone], &rules, &face, "r").is_none(), "隣が 0 の segment は通す");
    }

    /// 行 n5 (b) 越えの印の在る segment は照合せず oversize（row と裁定 id は読む上限の行）。隣が 0 の segment の越えは通し、断りは segment の順の最初で止まる。
    #[test]
    fn publish_scan_oversize_is_denied_without_matching() {
        let (rules, face) = (manifest(), Manifest::default());
        let denied = check(&[found("nothing", true)], &rules, &face, "r").unwrap_or_else(|| panic!("越えは断る"));
        let got = (denied.reason, denied.word.as_str(), denied.row, denied.ruling.as_str());
        assert_eq!(got, (Reason::Oversize, "host_guard.publish_read_bytes", "host_guard.publish_read_bytes", "r-host_guard.publish_read_bytes"));
        let quiet = Found { sighted: Sighted::default(), ..found("nothing", true) };
        assert!(check(std::slice::from_ref(&quiet), &rules, &face, "r").is_none(), "隣が 0 の周の越えは通す");
        let first = check(&[quiet, found("secret", true), found("secret", false)], &rules, &face, "r").map(|one| one.reason);
        assert_eq!(first, Some(Reason::Oversize), "segment の順に最初の断り");
    }
}
