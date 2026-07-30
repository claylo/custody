/// Collapse every non-empty run of Unicode whitespace to one ASCII space.
///
/// All non-whitespace characters are preserved exactly.
#[must_use]
pub fn normalize(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut pending_space = false;

    for character in input.chars() {
        if character.is_whitespace() {
            pending_space = !output.is_empty();
        } else {
            if pending_space {
                output.push(' ');
                pending_space = false;
            }
            output.push(character);
        }
    }

    output
}
