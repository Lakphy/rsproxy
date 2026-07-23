use super::*;

pub(super) fn flatten_conjunction<'a>(condition: &'a Condition, output: &mut Vec<&'a Condition>) {
    if let Condition::All(conditions) = condition {
        for condition in conditions {
            flatten_conjunction(condition, output);
        }
    } else {
        output.push(condition);
    }
}

pub(super) fn string_intersection<'a>(
    sets: impl Iterator<Item = &'a [String]>,
) -> Option<BTreeSet<&'a str>> {
    let mut intersection: Option<BTreeSet<&str>> = None;
    for values in sets {
        let values = values.iter().map(String::as_str).collect::<BTreeSet<_>>();
        intersection = Some(match intersection {
            Some(current) => current.intersection(&values).copied().collect(),
            None => values,
        });
    }
    intersection
}

pub(super) fn u16_intersection<'a>(sets: impl Iterator<Item = &'a [u16]>) -> Option<BTreeSet<u16>> {
    let mut intersection: Option<BTreeSet<u16>> = None;
    for values in sets {
        let values = values.iter().copied().collect::<BTreeSet<_>>();
        intersection = Some(match intersection {
            Some(current) => current.intersection(&values).copied().collect(),
            None => values,
        });
    }
    intersection
}
