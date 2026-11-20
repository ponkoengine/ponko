//! Rectangle packing: charts into a mesh's lightmap square, and models'
//! lightmaps into pages.

/// Places `sizes` (width, height) in a `width` x `height` area, tallest
/// first, in rows: where each goes (x, y, 0), or `None` if they don't fit.
pub(super) fn shelf_pack(sizes: &[(u32, u32)], width: u32, height: u32) -> Option<Vec<(u32, u32, u32)>> {
    let mut places = vec![(0, 0, 0); sizes.len()];
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(sizes[i].1), std::cmp::Reverse(sizes[i].0), i));
    let (mut x, mut y, mut row) = (0u32, 0u32, 0u32);
    for i in order {
        let (w, h) = sizes[i];
        if w > width || h > height {
            return None;
        }
        if x + w > width {
            x = 0;
            y += row;
            row = 0;
        }
        if y + h > height {
            return None;
        }
        places[i] = (x, y, 0);
        x += w;
        row = row.max(h);
    }
    Some(places)
}

/// Places `sizes` on as many `side` x `side` pages as they need, tallest
/// first, in rows, filling each page before the next: (x, y, page) each.
/// Every size must fit on a page.
pub(super) fn pack_pages(sizes: &[(u32, u32)], side: u32) -> (Vec<(u32, u32, u32)>, u32) {
    let mut places = vec![(0, 0, 0); sizes.len()];
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(sizes[i].1), std::cmp::Reverse(sizes[i].0), i));
    let (mut x, mut y, mut row, mut page) = (0u32, 0u32, 0u32, 0u32);
    for i in order {
        let (w, h) = (sizes[i].0.min(side), sizes[i].1.min(side));
        if x + w > side {
            x = 0;
            y += row;
            row = 0;
        }
        if y + h > side {
            page += 1;
            x = 0;
            y = 0;
            row = 0;
        }
        places[i] = (x, y, page);
        x += w;
        row = row.max(h);
    }
    let pages = if sizes.is_empty() { 0 } else { page + 1 };
    (places, pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overlap(a: (u32, u32, u32), sa: (u32, u32), b: (u32, u32, u32), sb: (u32, u32)) -> bool {
        a.2 == b.2 && a.0 < b.0 + sb.0 && b.0 < a.0 + sa.0 && a.1 < b.1 + sb.1 && b.1 < a.1 + sa.1
    }

    #[test]
    fn packed_rectangles_never_overlap_and_stay_on_their_page() {
        let sizes: Vec<(u32, u32)> = (0..60).map(|i| (5 + (i * 7) % 23, 4 + (i * 11) % 19)).collect();
        let (places, pages) = pack_pages(&sizes, 64);
        assert!(pages > 1, "60 rectangles of about 15x13 need more than one 64x64 page");
        for i in 0..sizes.len() {
            let (x, y, p) = places[i];
            assert!(x + sizes[i].0 <= 64 && y + sizes[i].1 <= 64 && p < pages);
            for j in 0..i {
                assert!(
                    !overlap(places[i], sizes[i], places[j], sizes[j]),
                    "{i} and {j} overlap"
                );
            }
        }
        assert!(shelf_pack(&[(10, 10); 4], 20, 20).is_some());
        assert!(shelf_pack(&[(10, 10); 5], 20, 20).is_none());
    }
}
