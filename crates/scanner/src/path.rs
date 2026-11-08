use std::path::Path;

pub fn join(base: &str, child: &str) -> String {
    let separator = if base.contains('\\') && !base.contains('/') {
        '\\'
    } else {
        '/'
    };
    format!(
        "{}{}{}",
        base.trim_end_matches(['/', '\\']),
        separator,
        child.trim_matches(['/', '\\'])
    )
}

pub fn normalize(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_owned()
}

pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && Path::new(name).components().count() == 1
}
