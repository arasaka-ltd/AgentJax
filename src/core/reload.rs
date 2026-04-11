use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ReloadDisposition {
    #[default]
    NoOp,
    HotReloadSafe,
    DrainAndSwap,
    RestartRequired,
}

impl ReloadDisposition {
    pub fn combine(self, other: Self) -> Self {
        self.max(other)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DrainDirective {
    pub module: String,
    pub strategy: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReloadInstruction {
    pub module: String,
    pub reason: String,
    pub disposition: ReloadDisposition,
    pub requires_drain: bool,
    pub config_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReloadPlan {
    pub snapshot_version: String,
    pub fingerprint_before: String,
    pub fingerprint_after: String,
    pub disposition: ReloadDisposition,
    pub affected_modules: Vec<String>,
    pub instructions: Vec<ReloadInstruction>,
    pub drained_modules: Vec<DrainDirective>,
    pub restart_required: bool,
}

impl ReloadPlan {
    pub fn push(&mut self, instruction: ReloadInstruction) {
        self.disposition = self
            .disposition
            .clone()
            .combine(instruction.disposition.clone());
        if instruction.requires_drain {
            self.drained_modules.push(DrainDirective {
                module: instruction.module.clone(),
                strategy: "prepare-health-check-swap-drain-shutdown".into(),
                reason: instruction.reason.clone(),
            });
        }
        self.affected_modules.push(instruction.module.clone());
        self.instructions.push(instruction);
        self.restart_required |= matches!(self.disposition, ReloadDisposition::RestartRequired);
    }

    pub fn finalize(&mut self) {
        self.affected_modules.sort();
        self.affected_modules.dedup();
        self.drained_modules
            .sort_by(|left, right| left.module.cmp(&right.module));
        self.drained_modules
            .dedup_by(|left, right| left.module == right.module && left.reason == right.reason);
        self.restart_required = matches!(self.disposition, ReloadDisposition::RestartRequired);
    }
}
