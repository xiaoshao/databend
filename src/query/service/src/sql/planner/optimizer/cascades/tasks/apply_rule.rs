// Copyright 2022 Datafuse Labs.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::rc::Rc;

use common_exception::Result;
use common_planner::IndexType;

use crate::sql::optimizer::cascades::tasks::SharedCounter;
use crate::sql::optimizer::cascades::CascadesOptimizer;
use crate::sql::optimizer::rule::TransformResult;
use crate::sql::optimizer::RuleFactory;
use crate::sql::optimizer::RuleID;

#[derive(Debug)]
pub struct ApplyRuleTask {
    pub rule_id: RuleID,
    pub target_group_index: IndexType,
    pub m_expr_index: IndexType,

    pub parent: Option<Rc<SharedCounter>>,
}

impl ApplyRuleTask {
    pub fn new(rule_id: RuleID, target_group_index: IndexType, m_expr_index: IndexType) -> Self {
        Self {
            rule_id,
            target_group_index,
            m_expr_index,
            parent: None,
        }
    }

    pub fn with_parent(
        rule_id: RuleID,
        target_group_index: IndexType,
        m_expr_index: IndexType,
        parent: &Rc<SharedCounter>,
    ) -> Self {
        let mut task = Self::new(rule_id, target_group_index, m_expr_index);
        parent.inc();
        task.parent = Some(parent.clone());
        task
    }

    pub fn execute(self, optimizer: &mut CascadesOptimizer) -> Result<()> {
        let group = optimizer.memo.group(self.target_group_index)?;
        let m_expr = group.m_expr(self.m_expr_index)?;
        let mut state = TransformResult::new();
        let rule = RuleFactory::create().create_rule(self.rule_id)?;
        m_expr.apply_rule(&optimizer.memo, &rule, &mut state)?;
        optimizer.insert_from_transform_state(self.target_group_index, state)?;

        if let Some(parent) = self.parent {
            parent.dec();
        }

        Ok(())
    }
}
