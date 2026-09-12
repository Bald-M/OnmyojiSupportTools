use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::device::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Point {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct FrameBounds {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClickTarget {
    Point {
        x: u32,
        y: u32,
    },
    Rect {
        left: u32,
        top: u32,
        width: u32,
        height: u32,
    },
}

impl From<Point> for ClickTarget {
    fn from(point: Point) -> Self {
        Self::Point {
            x: point.x,
            y: point.y,
        }
    }
}

pub fn sample_target<R: Rng + ?Sized>(
    target: &ClickTarget,
    frame: FrameBounds,
    radius: u32,
    previous: Option<Point>,
    rng: &mut R,
) -> Result<Point, AppError> {
    let mut candidates = Vec::new();
    match *target {
        ClickTarget::Point { x, y } => {
            if x >= frame.width || y >= frame.height || radius == 0 {
                return Err(AppError::new(
                    "CLICK_TARGET_INVALID",
                    "点击点或偏移半径无效",
                    "请在截图内重新选择并检查点击配置",
                ));
            }
            let minimum_x = x.saturating_sub(radius);
            let maximum_x = x.saturating_add(radius).min(frame.width.saturating_sub(1));
            let minimum_y = y.saturating_sub(radius);
            let maximum_y = y.saturating_add(radius).min(frame.height.saturating_sub(1));
            let radius_squared = u64::from(radius) * u64::from(radius);
            for candidate_y in minimum_y..=maximum_y {
                for candidate_x in minimum_x..=maximum_x {
                    let dx = i64::from(candidate_x) - i64::from(x);
                    let dy = i64::from(candidate_y) - i64::from(y);
                    let point = Point {
                        x: candidate_x,
                        y: candidate_y,
                    };
                    if point != (Point { x, y })
                        && Some(point) != previous
                        && (dx * dx + dy * dy) as u64 <= radius_squared
                    {
                        candidates.push(point);
                    }
                }
            }
        }
        ClickTarget::Rect {
            left,
            top,
            width,
            height,
        } => {
            let right = left.checked_add(width);
            let bottom = top.checked_add(height);
            if width == 0
                || height == 0
                || right.is_none_or(|v| v > frame.width)
                || bottom.is_none_or(|v| v > frame.height)
            {
                return Err(AppError::new(
                    "CLICK_TARGET_INVALID",
                    "点击区域无效或超出截图范围",
                    "请在截图内重新框选区域",
                ));
            }
            for y in top..bottom.unwrap() {
                for x in left..right.unwrap() {
                    let point = Point { x, y };
                    if Some(point) != previous {
                        candidates.push(point);
                    }
                }
            }
        }
    }
    if candidates.is_empty() {
        return Err(AppError::new(
            "CLICK_POINT_UNAVAILABLE",
            "无法生成不同且合法的最终落点",
            "请扩大点击区域或偏移半径",
        ));
    }
    Ok(candidates[rng.random_range(0..candidates.len())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn samples_legal_non_repeating_points() {
        let frame = FrameBounds {
            width: 10,
            height: 10,
        };
        let mut rng = StdRng::seed_from_u64(7);
        let point =
            sample_target(&ClickTarget::Point { x: 0, y: 0 }, frame, 6, None, &mut rng).unwrap();
        assert!(point.x < 10 && point.y < 10 && point != Point { x: 0, y: 0 });

        let rect = ClickTarget::Rect {
            left: 7,
            top: 8,
            width: 3,
            height: 2,
        };
        let first = sample_target(&rect, frame, 6, None, &mut rng).unwrap();
        let second = sample_target(&rect, frame, 6, Some(first), &mut rng).unwrap();
        assert_ne!(first, second);
        assert!((7..10).contains(&second.x) && (8..10).contains(&second.y));
    }

    #[test]
    fn rejects_invalid_or_unsampleable_targets() {
        let frame = FrameBounds {
            width: 1,
            height: 1,
        };
        let mut rng = StdRng::seed_from_u64(1);
        assert_eq!(
            sample_target(
                &ClickTarget::Rect {
                    left: 0,
                    top: 0,
                    width: 0,
                    height: 1
                },
                frame,
                1,
                None,
                &mut rng
            )
            .unwrap_err()
            .code,
            "CLICK_TARGET_INVALID"
        );
        assert_eq!(
            sample_target(&ClickTarget::Point { x: 0, y: 0 }, frame, 6, None, &mut rng)
                .unwrap_err()
                .code,
            "CLICK_POINT_UNAVAILABLE"
        );
    }
}
