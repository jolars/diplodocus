use super::{DocstringSourceSegment, SourceLocation, SourceSpan};

pub(super) fn validate(
    text: &str,
    source: &SourceLocation,
    segments: &[DocstringSourceSegment],
) -> Vec<DocstringSourceSegment> {
    let mut decoded_end = 0;
    let mut source_end = 0;
    for segment in segments {
        let decoded = segment.decoded;
        let original = segment.source;
        if decoded.start > decoded.end
            || decoded.end > text.len()
            || !text.is_char_boundary(decoded.start)
            || !text.is_char_boundary(decoded.end)
            || original.start > original.end
            || decoded.end - decoded.start != original.end - original.start
            || decoded.start < decoded_end
            || original.start < source_end
            || source
                .span
                .is_some_and(|s| original.start < s.start || original.end > s.end)
        {
            return Vec::new();
        }
        decoded_end = decoded.end;
        source_end = original.end;
    }
    segments.to_vec()
}

pub(super) fn complete(text: &str, segments: &[DocstringSourceSegment]) -> bool {
    let mut end = 0;
    for segment in segments {
        if segment.decoded.start != end {
            return false;
        }
        end = segment.decoded.end;
    }
    end == text.len()
}

pub(super) fn original(
    range: SourceSpan,
    segments: &[DocstringSourceSegment],
) -> Option<SourceSpan> {
    for (index, segment) in segments.iter().enumerate() {
        if range.start < segment.decoded.start || range.start > segment.decoded.end {
            continue;
        }
        let start = segment.source.start + range.start - segment.decoded.start;
        let mut decoded_end = segment.decoded.end;
        let mut original_end = segment.source.end;
        for next in &segments[index + 1..] {
            if range.end <= decoded_end {
                break;
            }
            if next.decoded.start != decoded_end || next.source.start != original_end {
                break;
            }
            decoded_end = next.decoded.end;
            original_end = next.source.end;
        }
        if range.end <= decoded_end {
            return Some(SourceSpan {
                start,
                end: start + range.end - range.start,
            });
        }
    }
    None
}
