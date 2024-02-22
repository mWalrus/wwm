pub fn format(cmd: &'static [&'static str]) -> Option<(&'static str, &'static [&'static str])> {
    match cmd.len() {
        0 => None,
        1 => Some((cmd[0], &[])),
        _ => Some((cmd[0], cmd.split_at(1).1)),
    }
}
