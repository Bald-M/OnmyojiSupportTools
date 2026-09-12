use image::{GenericImageView, ImageReader};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io::Cursor};

use crate::device::AppError;

pub const ACTIVITY_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameSpec {
    pub width: u32,
    pub height: u32,
    pub orientation: Orientation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Orientation {
    Landscape,
    Portrait,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn validate(self, frame: FrameSpec) -> Result<(), AppError> {
        let right = self.left.checked_add(self.width);
        let bottom = self.top.checked_add(self.height);
        if self.width == 0
            || self.height == 0
            || right.is_none_or(|v| v > frame.width)
            || bottom.is_none_or(|v| v > frame.height)
        {
            return Err(AppError::new(
                "ACTIVITY_REGION_INVALID",
                "活动区域无效或超出截图范围",
                "请重新框选截图内的区域",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PageState {
    ActivityEntry,
    StageEntry,
    Challenge,
    Battling,
    Reward,
    ReturnChallenge,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualFeature {
    pub region: Rect,
    /// Quantized 4x4 RGB means. These statistics cannot reconstruct the source pixels.
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateProfile {
    pub state: PageState,
    pub features: Vec<VisualFeature>,
    pub action: Option<Rect>,
    #[serde(default)]
    pub click_delay: Option<TimingRange>,
    #[serde(default)]
    pub press_duration: Option<TimingRange>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchPolicy {
    pub threshold: f32,
    pub minimum_margin: f32,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingRange {
    pub minimum_ms: u64,
    pub maximum_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownPopup {
    pub name: String,
    pub features: Vec<VisualFeature>,
    pub close_action: Rect,
    #[serde(default)]
    pub click_delay: Option<TimingRange>,
    #[serde(default)]
    pub press_duration: Option<TimingRange>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityConfig {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub frame: FrameSpec,
    pub states: Vec<StateProfile>,
    pub known_popups: Vec<KnownPopup>,
    pub matching: MatchPolicy,
    pub click_delay: TimingRange,
    pub press_duration: TimingRange,
}

impl ActivityConfig {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.version != ACTIVITY_CONFIG_VERSION {
            return Err(AppError::new(
                "ACTIVITY_CONFIG_VERSION",
                "不支持此活动配置版本",
                "请删除或重新校准该配置",
            ));
        }
        if self.id.trim().is_empty()
            || self.name.trim().is_empty()
            || self.frame.width == 0
            || self.frame.height == 0
        {
            return Err(AppError::new(
                "ACTIVITY_CONFIG_INVALID",
                "活动配置名称或画面尺寸无效",
                "请重新填写配置",
            ));
        }
        if !(0.0..=1.0).contains(&self.matching.threshold)
            || !(0.0..=1.0).contains(&self.matching.minimum_margin)
            || self.click_delay.minimum_ms > self.click_delay.maximum_ms
            || self.press_duration.minimum_ms == 0
            || self.press_duration.minimum_ms > self.press_duration.maximum_ms
        {
            return Err(AppError::new(
                "ACTIVITY_CONFIG_INVALID",
                "活动阈值或时序范围无效",
                "请修正配置范围",
            ));
        }
        let required: HashSet<_> = [
            PageState::ActivityEntry,
            PageState::StageEntry,
            PageState::Challenge,
            PageState::Battling,
            PageState::Reward,
            PageState::ReturnChallenge,
        ]
        .into_iter()
        .collect();
        let present: HashSet<_> = self.states.iter().map(|profile| profile.state).collect();
        if present != required || self.states.len() != required.len() {
            return Err(AppError::new(
                "ACTIVITY_CONFIG_INCOMPLETE",
                "活动配置必须完整且不重复地校准所有页面状态",
                "请完成六类页面校准",
            ));
        }
        for profile in &self.states {
            if profile.features.is_empty() {
                return Err(AppError::new(
                    "ACTIVITY_CONFIG_INVALID",
                    "每个页面状态至少需要一个识别区域",
                    "请完成页面校准",
                ));
            }
            for feature in &profile.features {
                feature.region.validate(self.frame)?;
                if feature.signature.len() != 48 {
                    return Err(AppError::new(
                        "ACTIVITY_FEATURE_INVALID",
                        "视觉特征格式无效",
                        "请重新校准该区域",
                    ));
                }
            }
            if let Some(action) = profile.action {
                action.validate(self.frame)?;
            }
            if profile.state != PageState::Battling && profile.action.is_none() {
                return Err(AppError::new(
                    "ACTIVITY_CONFIG_INCOMPLETE",
                    "可操作页面缺少安全动作区域",
                    "请为入口、关卡、首次挑战、奖励和返回挑战页设置动作区域",
                ));
            }
            for timing in [profile.click_delay, profile.press_duration]
                .into_iter()
                .flatten()
            {
                if timing.minimum_ms > timing.maximum_ms || timing.maximum_ms == 0 {
                    return Err(AppError::new(
                        "ACTIVITY_CONFIG_INVALID",
                        "逐动作时序覆盖无效",
                        "请修正该页面动作的时序范围",
                    ));
                }
            }
        }
        for popup in &self.known_popups {
            if popup.features.is_empty() {
                return Err(AppError::new(
                    "ACTIVITY_CONFIG_INVALID",
                    "已知弹窗缺少识别区域",
                    "请重新校准弹窗",
                ));
            }
            popup.close_action.validate(self.frame)?;
            for feature in &popup.features {
                feature.region.validate(self.frame)?;
            }
            for timing in [popup.click_delay, popup.press_duration]
                .into_iter()
                .flatten()
            {
                if timing.minimum_ms > timing.maximum_ms || timing.maximum_ms == 0 {
                    return Err(AppError::new(
                        "ACTIVITY_CONFIG_INVALID",
                        "弹窗关闭时序覆盖无效",
                        "请修正弹窗动作时序范围",
                    ));
                }
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn example_for_test() -> Self {
        let frame = FrameSpec {
            width: 1280,
            height: 720,
            orientation: Orientation::Landscape,
        };
        let feature = VisualFeature {
            region: Rect {
                left: 0,
                top: 0,
                width: 16,
                height: 16,
            },
            signature: vec![0; 48],
        };
        let states = [
            PageState::ActivityEntry,
            PageState::StageEntry,
            PageState::Challenge,
            PageState::Battling,
            PageState::Reward,
            PageState::ReturnChallenge,
        ]
        .into_iter()
        .map(|state| StateProfile {
            state,
            features: vec![feature.clone()],
            action: (state != PageState::Battling).then_some(feature.region),
            click_delay: None,
            press_duration: None,
        })
        .collect();
        Self {
            version: 1,
            id: "config".into(),
            name: "test".into(),
            frame,
            states,
            known_popups: vec![],
            matching: MatchPolicy {
                threshold: 0.9,
                minimum_margin: 0.1,
            },
            click_delay: TimingRange {
                minimum_ms: 300,
                maximum_ms: 900,
            },
            press_duration: TimingRange {
                minimum_ms: 45,
                maximum_ms: 120,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateScore {
    pub state: PageState,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecognitionResult {
    pub matched_state: Option<PageState>,
    pub scores: Vec<CandidateScore>,
    pub reason: Option<String>,
}

fn decode_png(bytes: &[u8]) -> Result<image::DynamicImage, AppError> {
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| {
            AppError::new(
                "ACTIVITY_IMAGE_INVALID",
                "无法读取校准画面",
                "请刷新截图后重试",
            )
        })?
        .decode()
        .map_err(|_| {
            AppError::new(
                "ACTIVITY_IMAGE_INVALID",
                "校准画面不是有效 PNG",
                "请刷新截图后重试",
            )
        })
}

pub fn extract_feature(png: &[u8], region: Rect) -> Result<VisualFeature, AppError> {
    let image = decode_png(png)?;
    let frame = FrameSpec {
        width: image.width(),
        height: image.height(),
        orientation: orientation(image.width(), image.height()),
    };
    region.validate(frame)?;
    let mut signature = Vec::with_capacity(48);
    for grid_y in 0..4 {
        for grid_x in 0..4 {
            let x0 = region.left + region.width * grid_x / 4;
            let x1 = region.left + region.width * (grid_x + 1) / 4;
            let y0 = region.top + region.height * grid_y / 4;
            let y1 = region.top + region.height * (grid_y + 1) / 4;
            let mut sums = [0u64; 3];
            let mut count = 0u64;
            for y in y0..y1.max(y0 + 1).min(region.top + region.height) {
                for x in x0..x1.max(x0 + 1).min(region.left + region.width) {
                    let pixel = image.get_pixel(x, y).0;
                    for channel in 0..3 {
                        sums[channel] += u64::from(pixel[channel]);
                    }
                    count += 1;
                }
            }
            for sum in sums {
                signature.push((sum / count.max(1)) as u8);
            }
        }
    }
    Ok(VisualFeature { region, signature })
}

fn orientation(width: u32, height: u32) -> Orientation {
    if width >= height {
        Orientation::Landscape
    } else {
        Orientation::Portrait
    }
}

fn feature_score(actual: &VisualFeature, expected: &VisualFeature) -> f32 {
    let difference: u32 = actual
        .signature
        .iter()
        .zip(&expected.signature)
        .map(|(a, b)| u32::from(a.abs_diff(*b)))
        .sum();
    1.0 - difference as f32 / (expected.signature.len() as f32 * 255.0)
}

fn score_features(png: &[u8], features: &[VisualFeature]) -> Result<f32, AppError> {
    let total: f32 = features
        .iter()
        .map(|expected| {
            extract_feature(png, expected.region).map(|actual| feature_score(&actual, expected))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .sum();
    Ok(total / features.len() as f32)
}

pub fn recognize(png: &[u8], config: &ActivityConfig) -> Result<RecognitionResult, AppError> {
    config.validate()?;
    let image = decode_png(png)?;
    if image.width() != config.frame.width
        || image.height() != config.frame.height
        || orientation(image.width(), image.height()) != config.frame.orientation
    {
        return Err(AppError::new(
            "ACTIVITY_FRAME_MISMATCH",
            "当前画面尺寸或方向与校准配置不符",
            "请切回校准设备设置或重新校准",
        ));
    }
    let mut scores = Vec::with_capacity(config.states.len());
    for profile in &config.states {
        scores.push(CandidateScore {
            state: profile.state,
            score: score_features(png, &profile.features)?,
        });
    }
    scores.sort_by(|a, b| b.score.total_cmp(&a.score));
    let best = scores.first();
    let second = scores.get(1).map_or(0.0, |score| score.score);
    let matched_state = best
        .filter(|score| {
            score.score >= config.matching.threshold
                && score.score - second >= config.matching.minimum_margin
        })
        .map(|score| score.state);
    let reason = matched_state.is_none().then(|| {
        if best.is_none_or(|score| score.score < config.matching.threshold) {
            "lowConfidence".to_owned()
        } else {
            "ambiguous".to_owned()
        }
    });
    Ok(RecognitionResult {
        matched_state,
        scores,
        reason,
    })
}

pub fn recognize_popup<'a>(
    png: &[u8],
    config: &'a ActivityConfig,
) -> Result<Option<&'a KnownPopup>, AppError> {
    let mut matches = Vec::new();
    for popup in &config.known_popups {
        if score_features(png, &popup.features)? >= config.matching.threshold {
            matches.push(popup);
        }
    }
    Ok((matches.len() == 1).then(|| matches[0]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Idle,
    Navigating,
    Ready,
    Starting,
    Battling,
    Rewarding,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySession {
    pub config_id: String,
    pub status: TaskStatus,
    pub current_state: Option<PageState>,
    pub target_runs: u32,
    pub completed_runs: u32,
    pub retry_count: u8,
    pub pause_reason: Option<PauseReason>,
    pub last_safe_action: Option<String>,
    pub last_event: Option<TaskEvent>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseReason {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub kind: String,
    pub detail: String,
}

impl ActivitySession {
    pub fn idle() -> Self {
        Self {
            config_id: String::new(),
            status: TaskStatus::Idle,
            current_state: None,
            target_runs: 0,
            completed_runs: 0,
            retry_count: 0,
            pause_reason: None,
            last_safe_action: None,
            last_event: None,
        }
    }

    pub fn start(config_id: String, target_runs: u32) -> Result<Self, AppError> {
        if config_id.trim().is_empty() || target_runs == 0 {
            return Err(AppError::new(
                "ACTIVITY_START_INVALID",
                "请选择活动配置并设置正数目标次数",
                "请修正任务参数",
            ));
        }
        Ok(Self {
            config_id,
            status: TaskStatus::Navigating,
            current_state: None,
            target_runs,
            completed_runs: 0,
            retry_count: 0,
            pause_reason: None,
            last_safe_action: None,
            last_event: Some(TaskEvent {
                kind: "started".into(),
                detail: "activity session started".into(),
            }),
        })
    }

    pub fn observe(&mut self, state: PageState) -> Result<(), AppError> {
        if matches!(
            self.status,
            TaskStatus::Paused | TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Idle
        ) {
            return Err(AppError::new(
                "ACTIVITY_TRANSITION_INVALID",
                "当前任务状态不能继续转换",
                "请开始新任务或确认恢复",
            ));
        }
        let previous = self.current_state;
        let valid = matches!(
            (previous, state),
            (None, PageState::ActivityEntry)
                | (Some(PageState::ActivityEntry), PageState::StageEntry)
                | (Some(PageState::StageEntry), PageState::Challenge)
                | (Some(PageState::Challenge), PageState::Battling)
                | (Some(PageState::Battling), PageState::Reward)
                | (Some(PageState::Reward), PageState::ReturnChallenge)
                | (Some(PageState::ReturnChallenge), PageState::Battling)
        );
        if !valid {
            return Err(AppError::new(
                "ACTIVITY_TRANSITION_INVALID",
                "识别到的页面不符合安全状态转换",
                "任务已停止输入，请检查当前页面",
            ));
        }
        self.status = match state {
            PageState::ActivityEntry | PageState::StageEntry => TaskStatus::Navigating,
            PageState::Challenge | PageState::ReturnChallenge => TaskStatus::Ready,
            PageState::Battling => TaskStatus::Battling,
            PageState::Reward => TaskStatus::Rewarding,
        };
        if previous == Some(PageState::Reward) && state == PageState::ReturnChallenge {
            self.completed_runs += 1;
            if self.completed_runs >= self.target_runs {
                self.status = TaskStatus::Completed;
            }
        }
        self.current_state = Some(state);
        self.retry_count = 0;
        self.last_event = Some(TaskEvent {
            kind: "stateObserved".into(),
            detail: format!("{:?}", state),
        });
        Ok(())
    }

    pub fn pause(&mut self, reason: impl Into<String>) {
        let message = reason.into();
        self.status = TaskStatus::Paused;
        self.pause_reason = Some(PauseReason {
            code: "safetyPause".into(),
            message: message.clone(),
        });
        self.last_event = Some(TaskEvent {
            kind: "paused".into(),
            detail: message,
        });
    }
}

impl Default for ActivitySession {
    fn default() -> Self {
        Self::idle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use std::io::Cursor;

    fn solid_png(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(width, height, Rgb(color)));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn rejects_unknown_versions_and_out_of_bounds_regions() {
        let mut config = ActivityConfig::example_for_test();
        config.version = 2;
        assert_eq!(
            config.validate().unwrap_err().code,
            "ACTIVITY_CONFIG_VERSION"
        );

        config.version = ACTIVITY_CONFIG_VERSION;
        config.states[0].features[0].region.left = config.frame.width;
        assert_eq!(
            config.validate().unwrap_err().code,
            "ACTIVITY_REGION_INVALID"
        );
    }

    #[test]
    fn counts_only_after_reward_returns_to_challenge() {
        let mut session = ActivitySession::start("config".into(), 1).unwrap();
        session.observe(PageState::ActivityEntry).unwrap();
        session.observe(PageState::StageEntry).unwrap();
        session.observe(PageState::Challenge).unwrap();
        session.observe(PageState::Battling).unwrap();
        session.observe(PageState::Reward).unwrap();
        assert_eq!(session.completed_runs, 0);
        session.observe(PageState::ReturnChallenge).unwrap();
        assert_eq!(session.completed_runs, 1);
        assert_eq!(session.status, TaskStatus::Completed);
    }

    #[test]
    fn recognizes_one_state_and_refuses_ambiguity_or_wrong_dimensions() {
        let red = solid_png(16, 16, [240, 10, 10]);
        let blue = solid_png(16, 16, [10, 10, 240]);
        let region = Rect {
            left: 0,
            top: 0,
            width: 16,
            height: 16,
        };
        let red_feature = extract_feature(&red, region).unwrap();
        let blue_feature = extract_feature(&blue, region).unwrap();
        assert_eq!(red_feature.signature.len(), 48);

        let mut config = ActivityConfig::example_for_test();
        config.frame = FrameSpec {
            width: 16,
            height: 16,
            orientation: Orientation::Landscape,
        };
        config.matching = MatchPolicy {
            threshold: 0.95,
            minimum_margin: 0.1,
        };
        config.states[0].features = vec![red_feature.clone()];
        for profile in &mut config.states[1..] {
            profile.features = vec![blue_feature.clone()];
        }
        assert_eq!(
            recognize(&red, &config).unwrap().matched_state,
            Some(PageState::ActivityEntry)
        );

        config.states[1].features = vec![red_feature];
        assert_eq!(recognize(&red, &config).unwrap().matched_state, None);
        assert_eq!(
            recognize(&solid_png(8, 8, [240, 10, 10]), &config)
                .unwrap_err()
                .code,
            "ACTIVITY_FRAME_MISMATCH"
        );
    }
}
