use image::{GenericImageView, ImageReader};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io::Cursor};

use crate::device::AppError;

pub const ACTIVITY_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    #[default]
    Generic,
    RealmRaid,
}

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
    Opponent,
    BattleReady,
    Battling,
    Reward,
    Defeat,
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
pub struct FeatureAction {
    pub features: Vec<VisualFeature>,
    pub action: Option<Rect>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmRaidOpponent {
    pub available_features: Vec<VisualFeature>,
    pub action: Rect,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmRaidPauseCondition {
    pub name: String,
    pub features: Vec<VisualFeature>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealmRaidConfig {
    pub opponents: Vec<RealmRaidOpponent>,
    pub refresh: FeatureAction,
    pub progress_rewards: Vec<FeatureAction>,
    pub attack_requirements: Vec<VisualFeature>,
    pub failure_limit: u8,
    #[serde(default)]
    pub pause_conditions: Vec<RealmRaidPauseCondition>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityConfig {
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: ActivityKind,
    pub frame: FrameSpec,
    pub states: Vec<StateProfile>,
    pub known_popups: Vec<KnownPopup>,
    pub matching: MatchPolicy,
    pub click_delay: TimingRange,
    pub press_duration: TimingRange,
    #[serde(default)]
    pub realm_raid: Option<RealmRaidConfig>,
}

fn validate_features(features: &[VisualFeature], frame: FrameSpec) -> Result<(), AppError> {
    for feature in features {
        feature.region.validate(frame)?;
        if feature.signature.len() != 48 {
            return Err(AppError::new(
                "ACTIVITY_FEATURE_INVALID",
                "视觉特征格式无效",
                "请重新校准该区域",
            ));
        }
    }
    Ok(())
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
        let required: HashSet<_> = match self.kind {
            ActivityKind::Generic => vec![
                PageState::ActivityEntry,
                PageState::StageEntry,
                PageState::Challenge,
                PageState::Battling,
                PageState::Reward,
                PageState::ReturnChallenge,
            ],
            ActivityKind::RealmRaid => vec![
                PageState::ActivityEntry,
                PageState::StageEntry,
                PageState::Challenge,
                PageState::Opponent,
                PageState::BattleReady,
                PageState::Battling,
                PageState::Reward,
                PageState::Defeat,
            ],
        }
        .into_iter()
        .collect();
        let present: HashSet<_> = self.states.iter().map(|profile| profile.state).collect();
        if present != required || self.states.len() != required.len() {
            return Err(AppError::new(
                "ACTIVITY_CONFIG_INCOMPLETE",
                "活动配置必须完整且不重复地校准所有页面状态",
                "请完成当前活动类型要求的全部页面校准",
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
            let uses_dynamic_action =
                self.kind == ActivityKind::RealmRaid && profile.state == PageState::Challenge;
            if profile.state != PageState::Battling
                && !uses_dynamic_action
                && profile.action.is_none()
            {
                return Err(AppError::new(
                    "ACTIVITY_CONFIG_INCOMPLETE",
                    "可操作页面缺少安全动作区域",
                    "请为当前活动中的每个可操作页面设置动作区域",
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
        match (&self.kind, &self.realm_raid) {
            (ActivityKind::Generic, None) => {}
            (ActivityKind::RealmRaid, Some(realm_raid)) => {
                if realm_raid.opponents.len() != 9 {
                    return Err(AppError::new(
                        "REALM_RAID_OPPONENTS_INVALID",
                        "结界突破必须配置九个对手区域",
                        "请按九宫格顺序依次框选全部对手",
                    ));
                }
                for opponent in &realm_raid.opponents {
                    if opponent.available_features.is_empty() {
                        return Err(AppError::new(
                            "REALM_RAID_OPPONENTS_INVALID",
                            "每个对手区域都需要可挑战视觉特征",
                            "请为九个对手分别添加可挑战特征",
                        ));
                    }
                    opponent.action.validate(self.frame)?;
                    validate_features(&opponent.available_features, self.frame)?;
                }
                if realm_raid.refresh.features.is_empty() {
                    return Err(AppError::new(
                        "REALM_RAID_REFRESH_INVALID",
                        "结界突破缺少可用刷新特征",
                        "请在刷新可用时添加识别区域",
                    ));
                }
                let refresh_action = realm_raid.refresh.action.ok_or_else(|| {
                    AppError::new(
                        "REALM_RAID_REFRESH_INVALID",
                        "结界突破缺少刷新动作区域",
                        "请框选刷新按钮",
                    )
                })?;
                refresh_action.validate(self.frame)?;
                validate_features(&realm_raid.refresh.features, self.frame)?;
                if realm_raid.progress_rewards.len() != 3 {
                    return Err(AppError::new(
                        "REALM_RAID_REWARDS_INVALID",
                        "结界突破必须配置 3/6/9 三个进度奖励",
                        "请依次配置三个可领取奖励区域",
                    ));
                }
                for reward in &realm_raid.progress_rewards {
                    if reward.features.is_empty() {
                        return Err(AppError::new(
                            "REALM_RAID_REWARDS_INVALID",
                            "进度奖励缺少可领取视觉特征",
                            "请在奖励可领取时添加识别区域",
                        ));
                    }
                    let reward_action = reward.action.ok_or_else(|| {
                        AppError::new(
                            "REALM_RAID_REWARDS_INVALID",
                            "进度奖励缺少动作区域",
                            "请重新框选奖励区域",
                        )
                    })?;
                    reward_action.validate(self.frame)?;
                    validate_features(&reward.features, self.frame)?;
                }
                if realm_raid.attack_requirements.len() < 2 {
                    return Err(AppError::new(
                        "REALM_RAID_ATTACK_INVALID",
                        "进攻前至少需要目标详情和消耗 1 两项特征",
                        "请在对手详情页添加两项进攻门禁特征",
                    ));
                }
                validate_features(&realm_raid.attack_requirements, self.frame)?;
                if !(1..=9).contains(&realm_raid.failure_limit) {
                    return Err(AppError::new(
                        "REALM_RAID_FAILURE_LIMIT_INVALID",
                        "连续失败上限必须在 1 到 9 之间",
                        "请修正安全暂停上限",
                    ));
                }
                for condition in &realm_raid.pause_conditions {
                    if condition.name.trim().is_empty() || condition.features.is_empty() {
                        return Err(AppError::new(
                            "REALM_RAID_PAUSE_CONDITION_INVALID",
                            "安全暂停条件缺少名称或视觉特征",
                            "请重新校准无票等暂停条件",
                        ));
                    }
                    validate_features(&condition.features, self.frame)?;
                }
            }
            (ActivityKind::Generic, Some(_)) | (ActivityKind::RealmRaid, None) => {
                return Err(AppError::new(
                    "REALM_RAID_CONFIG_INVALID",
                    "活动类型与结界突破配置不一致",
                    "请重新创建活动配置",
                ));
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
            kind: ActivityKind::Generic,
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
            realm_raid: None,
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

pub fn features_match(
    png: &[u8],
    features: &[VisualFeature],
    threshold: f32,
) -> Result<bool, AppError> {
    if features.is_empty() {
        return Ok(false);
    }
    features.iter().try_fold(true, |matched, expected| {
        if !matched {
            return Ok(false);
        }
        let actual = extract_feature(png, expected.region)?;
        Ok(feature_score(&actual, expected) >= threshold)
    })
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum SafeAction {
    Page(PageState),
    RealmRaidOpponent(u8),
    RealmRaidRefresh,
    RealmRaidProgressReward(u8),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivitySession {
    pub config_id: String,
    pub status: TaskStatus,
    pub current_state: Option<PageState>,
    pub target_runs: u32,
    pub completed_runs: u32,
    #[serde(default)]
    pub next_opponent_index: u8,
    #[serde(default)]
    pub attempted_opponents: Vec<u8>,
    #[serde(default)]
    pub failed_opponents: Vec<u8>,
    #[serde(default)]
    pub consecutive_failures: u8,
    pub retry_count: u8,
    pub pause_reason: Option<PauseReason>,
    pub last_safe_action: Option<SafeAction>,
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
            next_opponent_index: 0,
            attempted_opponents: Vec::new(),
            failed_opponents: Vec::new(),
            consecutive_failures: 0,
            retry_count: 0,
            pause_reason: None,
            last_safe_action: None,
            last_event: None,
        }
    }

    pub fn start(config_id: String, target_runs: u32) -> Result<Self, AppError> {
        if config_id.trim().is_empty() || !(1..=30).contains(&target_runs) {
            return Err(AppError::new(
                "ACTIVITY_TARGET_RUNS_INVALID",
                "目标成功次数必须在 1 到 30 之间",
                "请输入不超过挑战券持有上限的整数",
            ));
        }
        Ok(Self {
            config_id,
            status: TaskStatus::Navigating,
            current_state: None,
            target_runs,
            completed_runs: 0,
            next_opponent_index: 0,
            attempted_opponents: Vec::new(),
            failed_opponents: Vec::new(),
            consecutive_failures: 0,
            retry_count: 0,
            pause_reason: None,
            last_safe_action: None,
            last_event: Some(TaskEvent {
                kind: "started".into(),
                detail: "activity session started".into(),
            }),
        })
    }

    pub fn observe_for(&mut self, kind: ActivityKind, state: PageState) -> Result<(), AppError> {
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
        let valid = match kind {
            ActivityKind::Generic => matches!(
                (previous, state),
                (None, PageState::ActivityEntry)
                    | (Some(PageState::ActivityEntry), PageState::StageEntry)
                    | (Some(PageState::StageEntry), PageState::Challenge)
                    | (Some(PageState::Challenge), PageState::Battling)
                    | (Some(PageState::Battling), PageState::Reward)
                    | (Some(PageState::Reward), PageState::ReturnChallenge)
                    | (Some(PageState::ReturnChallenge), PageState::Battling)
            ),
            ActivityKind::RealmRaid => matches!(
                (previous, state),
                (None, PageState::ActivityEntry)
                    | (Some(PageState::ActivityEntry), PageState::StageEntry)
                    | (Some(PageState::StageEntry), PageState::Challenge)
                    | (Some(PageState::Challenge), PageState::Opponent)
                    | (Some(PageState::Opponent), PageState::BattleReady)
                    | (Some(PageState::BattleReady), PageState::Battling)
                    | (Some(PageState::Battling), PageState::Reward)
                    | (Some(PageState::Battling), PageState::Defeat)
                    | (Some(PageState::Reward), PageState::Challenge)
                    | (Some(PageState::Defeat), PageState::Challenge)
            ),
        };
        if !valid {
            return Err(AppError::new(
                "ACTIVITY_TRANSITION_INVALID",
                "识别到的页面不符合安全状态转换",
                "任务已停止输入，请检查当前页面",
            ));
        }
        self.status = match state {
            PageState::ActivityEntry | PageState::StageEntry => TaskStatus::Navigating,
            PageState::Challenge | PageState::ReturnChallenge | PageState::Opponent => {
                TaskStatus::Ready
            }
            PageState::BattleReady => TaskStatus::Starting,
            PageState::Battling => TaskStatus::Battling,
            PageState::Reward => TaskStatus::Rewarding,
            PageState::Defeat => TaskStatus::Rewarding,
        };
        let settled = previous == Some(PageState::Reward)
            && matches!(
                (kind, state),
                (ActivityKind::Generic, PageState::ReturnChallenge)
                    | (ActivityKind::RealmRaid, PageState::Challenge)
            );
        if settled {
            self.completed_runs += 1;
            self.consecutive_failures = 0;
            if self.completed_runs >= self.target_runs {
                self.status = TaskStatus::Completed;
            }
        } else if previous == Some(PageState::Battling) && state == PageState::Defeat {
            self.consecutive_failures = self.consecutive_failures.saturating_add(1);
            if let Some(index) = self.attempted_opponents.last().copied()
                && !self.failed_opponents.contains(&index)
            {
                self.failed_opponents.push(index);
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
        session
            .observe_for(ActivityKind::Generic, PageState::ActivityEntry)
            .unwrap();
        session
            .observe_for(ActivityKind::Generic, PageState::StageEntry)
            .unwrap();
        session
            .observe_for(ActivityKind::Generic, PageState::Challenge)
            .unwrap();
        session
            .observe_for(ActivityKind::Generic, PageState::Battling)
            .unwrap();
        session
            .observe_for(ActivityKind::Generic, PageState::Reward)
            .unwrap();
        assert_eq!(session.completed_runs, 0);
        session
            .observe_for(ActivityKind::Generic, PageState::ReturnChallenge)
            .unwrap();
        assert_eq!(session.completed_runs, 1);
        assert_eq!(session.status, TaskStatus::Completed);
    }

    #[test]
    fn limits_a_task_to_the_thirty_ticket_capacity() {
        assert!(ActivitySession::start("config".into(), 30).is_ok());
        assert_eq!(
            ActivitySession::start("config".into(), 31)
                .unwrap_err()
                .code,
            "ACTIVITY_TARGET_RUNS_INVALID"
        );
    }

    #[test]
    fn realm_raid_counts_wins_but_not_defeats() {
        let mut session = ActivitySession::start("config".into(), 2).unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::ActivityEntry)
            .unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::StageEntry)
            .unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::Challenge)
            .unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::Opponent)
            .unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::BattleReady)
            .unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::Battling)
            .unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::Defeat)
            .unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::Challenge)
            .unwrap();
        assert_eq!(session.completed_runs, 0);
        assert_eq!(session.consecutive_failures, 1);

        session
            .observe_for(ActivityKind::RealmRaid, PageState::Opponent)
            .unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::BattleReady)
            .unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::Battling)
            .unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::Reward)
            .unwrap();
        session
            .observe_for(ActivityKind::RealmRaid, PageState::Challenge)
            .unwrap();
        assert_eq!(session.completed_runs, 1);
        assert_eq!(session.consecutive_failures, 0);
        assert_ne!(session.status, TaskStatus::Completed);
    }

    #[test]
    fn realm_raid_requires_nine_opponents_and_a_refresh_action() {
        let mut config = ActivityConfig::example_for_test();
        config.kind = ActivityKind::RealmRaid;
        config.states = [
            PageState::ActivityEntry,
            PageState::StageEntry,
            PageState::Challenge,
            PageState::Opponent,
            PageState::BattleReady,
            PageState::Battling,
            PageState::Reward,
            PageState::Defeat,
        ]
        .into_iter()
        .map(|state| StateProfile {
            state,
            features: config.states[0].features.clone(),
            action: (state != PageState::Battling).then_some(config.states[0].features[0].region),
            click_delay: None,
            press_duration: None,
        })
        .collect();
        config.realm_raid = Some(RealmRaidConfig {
            opponents: vec![
                RealmRaidOpponent {
                    available_features: config.states[0].features.clone(),
                    action: config.states[0].features[0].region,
                };
                8
            ],
            refresh: FeatureAction {
                features: config.states[0].features.clone(),
                action: Some(config.states[0].features[0].region),
            },
            progress_rewards: vec![
                FeatureAction {
                    features: config.states[0].features.clone(),
                    action: Some(config.states[0].features[0].region),
                };
                3
            ],
            attack_requirements: vec![config.states[0].features[0].clone(); 2],
            failure_limit: 3,
            pause_conditions: vec![],
        });

        assert_eq!(
            config.validate().unwrap_err().code,
            "REALM_RAID_OPPONENTS_INVALID"
        );
        config
            .realm_raid
            .as_mut()
            .unwrap()
            .opponents
            .push(RealmRaidOpponent {
                available_features: config.states[0].features.clone(),
                action: config.states[0].features[0].region,
            });
        assert!(config.validate().is_ok());
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
