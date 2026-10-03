use serde::de::{Deserialize, Error, IgnoredAny, SeqAccess};

pub(crate) fn required<'de, T: Deserialize<'de>, A: SeqAccess<'de>>(
    seq: &mut A,
) -> Result<T, A::Error> {
    seq.next_element()?
        .ok_or_else(|| A::Error::custom("missing expression argument"))
}

pub(crate) fn finish<'de, A: SeqAccess<'de>>(seq: &mut A) -> Result<(), A::Error> {
    if seq.next_element::<IgnoredAny>()?.is_some() {
        Err(A::Error::custom("unexpected expression argument"))
    } else {
        Ok(())
    }
}

pub(crate) fn arguments<'de, T: Deserialize<'de>, A: SeqAccess<'de>>(
    seq: &mut A,
    minimum: usize,
) -> Result<Vec<T>, A::Error> {
    let mut args = Vec::new();
    while let Some(arg) = seq.next_element()? {
        args.push(arg);
    }
    if args.len() < minimum {
        return Err(A::Error::custom("expression has too few arguments"));
    }
    Ok(args)
}
