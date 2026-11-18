//! Bound declared archive expansion before invoking the external parser.
const EXPANDED_LIMIT: u64 = 256 * 1024 * 1024;
const ENTRY_LIMIT: u64 = 128 * 1024 * 1024;

fn word(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at.checked_add(2)?)?.try_into().ok()?))
}
fn long(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

pub(super) fn check(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("SKP files are limited to 64 MiB.".into());
    }
    match skpwright::detect_container(bytes) {
        skpwright::Container::Stream => return Ok(()),
        skpwright::Container::Unknown => return Err("The file is not a recognized SKP container.".into()),
        skpwright::Container::Zip => {}
    }
    let invalid = || "Invalid or unsupported SKP archive directory.".to_owned();
    let end = (bytes.len().saturating_sub(65_557)..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| {
            long(bytes, i) == Some(0x0605_4b50)
                && word(bytes, i + 20).is_some_and(|n| i + 22 + usize::from(n) == bytes.len())
        })
        .ok_or_else(invalid)?;
    let count = word(bytes, end + 10).ok_or_else(invalid)?;
    let directory_size = long(bytes, end + 12).ok_or_else(invalid)?;
    let directory_offset = long(bytes, end + 16).ok_or_else(invalid)?;
    if word(bytes, end + 4) != Some(0)
        || word(bytes, end + 6) != Some(0)
        || word(bytes, end + 8) != Some(count)
        || count > 10_000
        || directory_size == u32::MAX
        || directory_offset == u32::MAX
    {
        return Err(invalid());
    }
    let start = end.checked_sub(directory_size as usize).ok_or_else(invalid)?;
    start.checked_sub(directory_offset as usize).ok_or_else(invalid)?;
    let mut cursor = start;
    let mut total = 0_u64;
    for _ in 0..count {
        if cursor.checked_add(46).is_none_or(|limit| limit > end) || long(bytes, cursor) != Some(0x0201_4b50) {
            return Err(invalid());
        }
        let size = u64::from(long(bytes, cursor + 24).ok_or_else(invalid)?);
        total = total.checked_add(size).ok_or_else(invalid)?;
        if size > ENTRY_LIMIT || total > EXPANDED_LIMIT {
            return Err("SKP archive expansion exceeds the 128 MiB entry or 256 MiB total limit.".into());
        }
        let name = usize::from(word(bytes, cursor + 28).ok_or_else(invalid)?);
        let extra = usize::from(word(bytes, cursor + 30).ok_or_else(invalid)?);
        let comment = usize::from(word(bytes, cursor + 32).ok_or_else(invalid)?);
        cursor = cursor.checked_add(46 + name + extra + comment).filter(|&n| n <= end).ok_or_else(invalid)?;
    }
    if cursor != end {
        return Err(invalid());
    }
    Ok(())
}
