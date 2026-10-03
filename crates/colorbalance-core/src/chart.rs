//! ColorChecker chart model.
//!
//! The physical chart revision must be selected explicitly because the
//! 24-patch layout is identical across revisions while the reference values
//! differ. Reference values and their illuminant are added in milestone 1
//! issue 2 from independently verified datasets; none are invented here.

use serde::{Deserialize, Serialize};

/// Supported physical chart revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChartRevision {
    /// ColorChecker Classic manufactured before November 2014.
    ClassicBeforeNovember2014,
    /// ColorChecker Classic manufactured from November 2014 onward,
    /// including the Calibrite rebrand.
    ClassicFromNovember2014,
}

/// One patch of a 24-patch ColorChecker Classic, in reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChartPatch {
    DarkSkin,
    LightSkin,
    BlueSky,
    Foliage,
    BlueFlower,
    BluishGreen,
    Orange,
    PurplishBlue,
    ModerateRed,
    Purple,
    YellowGreen,
    OrangeYellow,
    Blue,
    Green,
    Red,
    Yellow,
    Magenta,
    Cyan,
    White,
    Neutral8,
    Neutral65,
    Neutral5,
    Neutral35,
    Black,
}

/// The chart model a profile was derived against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ChartModel {
    pub revision: ChartRevision,
    /// Neutral patches in reading order, used for exposure and channel
    /// scaling during derivation.
    pub neutral_patches: [ChartPatch; 6],
    /// All 24 patches in reading order.
    pub patches: [ChartPatch; 24],
}

impl ChartModel {
    /// Build the chart model for a revision.
    pub fn new(revision: ChartRevision) -> Self {
        Self {
            revision,
            neutral_patches: [
                ChartPatch::White,
                ChartPatch::Neutral8,
                ChartPatch::Neutral65,
                ChartPatch::Neutral5,
                ChartPatch::Neutral35,
                ChartPatch::Black,
            ],
            patches: [
                ChartPatch::DarkSkin,
                ChartPatch::LightSkin,
                ChartPatch::BlueSky,
                ChartPatch::Foliage,
                ChartPatch::BlueFlower,
                ChartPatch::BluishGreen,
                ChartPatch::Orange,
                ChartPatch::PurplishBlue,
                ChartPatch::ModerateRed,
                ChartPatch::Purple,
                ChartPatch::YellowGreen,
                ChartPatch::OrangeYellow,
                ChartPatch::Blue,
                ChartPatch::Green,
                ChartPatch::Red,
                ChartPatch::Yellow,
                ChartPatch::Magenta,
                ChartPatch::Cyan,
                ChartPatch::White,
                ChartPatch::Neutral8,
                ChartPatch::Neutral65,
                ChartPatch::Neutral5,
                ChartPatch::Neutral35,
                ChartPatch::Black,
            ],
        }
    }

    /// Index of a patch in reading order.
    pub fn index_of(&self, patch: ChartPatch) -> Option<usize> {
        self.patches
            .iter()
            .position(|candidate| *candidate == patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_has_24_unique_patches() {
        let chart = ChartModel::new(ChartRevision::ClassicFromNovember2014);
        assert_eq!(chart.patches.len(), 24);
        for (i, patch) in chart.patches.iter().enumerate() {
            assert_eq!(chart.index_of(*patch), Some(i));
            assert_eq!(chart.patches.iter().filter(|p| **p == *patch).count(), 1);
        }
    }

    #[test]
    fn neutral_patches_are_the_bottom_row_in_reading_order() {
        let chart = ChartModel::new(ChartRevision::ClassicBeforeNovember2014);
        assert_eq!(chart.neutral_patches[0], ChartPatch::White);
        assert_eq!(chart.neutral_patches[5], ChartPatch::Black);
        for patch in chart.neutral_patches {
            assert_eq!(
                chart.index_of(patch),
                chart.patches.iter().position(|p| *p == patch)
            );
        }
    }

    #[test]
    fn chart_model_round_trips_through_json() {
        let chart = ChartModel::new(ChartRevision::ClassicFromNovember2014);
        let json = serde_json::to_string(&chart).expect("serialize");
        let parsed: ChartModel = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, chart);
    }
}
