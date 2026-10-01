//! 先撃ちと後の行の審査の口が共用する、予想の base の実体化と材料の鍵（設計 docs/design/row-review.md §3 形 5・行 a0 の純移動）。

use super::super::dispatch::precheck::Layer;
use super::super::declaration::{table_facts, Ceiling, CEILING_ROW, DENIED_ROW};
use super::super::refuse::{normalize, DELETE_FILE, NEW_FILE};
use super::super::git_ok;
use crate::hook::vessel::digest::fnv1a_64;
use crate::rules::manifest::Manifest;
use std::path::{Path, PathBuf};

/// 層を依存の順に当てて index に載せ（`git add -A`）、宣言だけの祖先が空で置いた file の列を返す（宣言だけの祖先が無ければ
/// `None`＝予想の印を足さない）。Gated PASS の祖先は `add` を祖先の木の HEAD から拡張子で絞らずに写し `remove` を消す。
pub(in crate::pipe) fn materialize(tree: &Path, layers: &[Layer]) -> Result<Option<Vec<String>>, String> {
    let mut declared: Option<Vec<String>> = None;
    for layer in layers {
        match *layer {
            Layer::Tree { ref add, ref remove, ref head, .. } => {
                let mut args = vec!["checkout", head.as_str(), "--"];
                args.extend(add.iter().map(String::as_str));
                if !add.is_empty() && !git_ok(tree, &args) {
                    return Err(format!("祖先の木 {head} の file を写せない"));
                }
                for path in remove {
                    let _ = std::fs::remove_file(tree.join(path));
                }
            }
            Layer::Declared(ref write_set) => declare(tree, write_set, declared.get_or_insert_with(Vec::new))?,
        }
    }
    if !git_ok(tree, &["add", "-A"]) {
        return Err("予想の base を index に載せられない".to_owned());
    }
    Ok(declared)
}

/// 宣言だけの祖先の層: `+` の file を空で作り（`placed` に足す）、`~` の file を消す。
fn declare(tree: &Path, write_set: &[String], placed: &mut Vec<String>) -> Result<(), String> {
    for item in write_set {
        let path = normalize(item);
        if item.starts_with(DELETE_FILE) {
            let target = tree.join(&path);
            let _ = if path.ends_with('/') { std::fs::remove_dir_all(target) } else { std::fs::remove_file(target) };
        }
        if !item.starts_with(NEW_FILE) || path.ends_with('/') {
            continue;
        }
        let target = tree.join(&path);
        let parent = target.parent().map(Path::to_path_buf).unwrap_or_else(|| tree.to_path_buf());
        std::fs::create_dir_all(parent).and_then(|()| std::fs::write(&target, "")).map_err(|err| format!("{path} を置けない: {err}"))?;
        placed.push(path);
    }
    Ok(())
}

/// 要件面の repo 相対 path（HEAD の宣言 `requirements`・`pipe review` の入口と同じ [`table_facts`] の読み口・表の検査を撃たない
/// 組み立て＝クラスの語列表は空）。
pub(in crate::pipe) fn requirements_of(repo: &Path, manifest: &Manifest) -> Result<String, String> {
    let commands = crate::rules::list_row(manifest, CEILING_ROW)?;
    let denied = crate::rules::list_row(manifest, DENIED_ROW)?;
    let ceiling = Ceiling { row: CEILING_ROW, commands, denied, classes: &[] };
    table_facts(repo, &ceiling)
        .map(|facts| facts.requirements)
        .map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<String>>().join(" / "))
}

/// 材料の鍵（dir の全 file の名と本文を名の順に並べた digest の 1 つの字・材料の種類を列挙しない・形 aa 3）。
pub(in crate::pipe) fn digest(dir: &Path) -> Result<String, String> {
    let entries = std::fs::read_dir(dir).map_err(|err| format!("{} を読めない: {err}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    let mut bytes = Vec::new();
    for path in paths {
        bytes.extend(path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default().into_bytes());
        bytes.push(0);
        bytes.extend(std::fs::read(&path).map_err(|err| format!("{} を読めない: {err}", path.display()))?);
        bytes.push(0);
    }
    Ok(fnv1a_64(&bytes))
}
