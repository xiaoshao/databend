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

use std::sync::Arc;

use common_catalog::table_context::TableContext;
use common_exception::Result;
use common_functions::scalars::FunctionFactory;
use common_planner::IndexType;
use parking_lot::RwLock;

use crate::evaluator::EvalNode;
use crate::evaluator::Evaluator;
use crate::pipelines::processors::transforms::hash_join::row::RowPtr;
use crate::sessions::QueryContext;
use crate::sql::executor::HashJoin;
use crate::sql::executor::PhysicalScalar;
use crate::sql::plans::JoinType;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Hash)]
pub enum MarkerKind {
    True,
    False,
    Null,
}

pub struct MarkJoinDesc {
    pub(crate) marker_index: Option<IndexType>,
    pub(crate) has_null: RwLock<bool>,
}

pub struct RightJoinDesc {
    /// Record rows in build side that are matched with rows in probe side.
    /// It's order-sensitive, aligned with the order of rows in merged block.
    pub(crate) build_indexes: RwLock<Vec<RowPtr>>,
}

impl RightJoinDesc {
    pub fn create(ctx: Arc<QueryContext>) -> Result<Self> {
        let max_block_size = ctx.get_settings().get_max_block_size()? as usize;
        Ok(RightJoinDesc {
            build_indexes: RwLock::new(Vec::with_capacity(max_block_size)),
        })
    }
}

pub struct HashJoinDesc {
    pub(crate) build_keys: Vec<EvalNode>,
    pub(crate) probe_keys: Vec<EvalNode>,
    pub(crate) join_type: JoinType,
    pub(crate) other_predicate: Option<EvalNode>,
    pub(crate) marker_join_desc: MarkJoinDesc,
    /// Whether the Join are derived from correlated subquery.
    pub(crate) from_correlated_subquery: bool,
    pub(crate) right_join_desc: RightJoinDesc,
}

impl HashJoinDesc {
    pub fn create(ctx: Arc<QueryContext>, join: &HashJoin) -> Result<HashJoinDesc> {
        let predicate = Self::join_predicate(&join.other_conditions)?;

        Ok(HashJoinDesc {
            join_type: join.join_type.clone(),
            build_keys: Evaluator::eval_physical_scalars(&join.build_keys)?,
            probe_keys: Evaluator::eval_physical_scalars(&join.probe_keys)?,
            other_predicate: predicate
                .as_ref()
                .map(Evaluator::eval_physical_scalar)
                .transpose()?,
            marker_join_desc: MarkJoinDesc {
                has_null: RwLock::new(false),
                marker_index: join.marker_index,
            },
            from_correlated_subquery: join.from_correlated_subquery,
            right_join_desc: RightJoinDesc::create(ctx)?,
        })
    }

    fn join_predicate(other_conditions: &[PhysicalScalar]) -> Result<Option<PhysicalScalar>> {
        if other_conditions.is_empty() {
            return Ok(None);
        }

        let mut condition = other_conditions[0].clone();

        for other_condition in other_conditions.iter().skip(1) {
            let left_type = condition.data_type();
            let right_type = other_condition.data_type();
            let data_types = vec![&left_type, &right_type];
            let func = FunctionFactory::instance().get("and", &data_types)?;
            condition = PhysicalScalar::Function {
                name: "and".to_string(),
                args: vec![
                    (condition, left_type),
                    (other_condition.clone(), right_type),
                ],
                return_type: func.return_type(),
            };
        }

        Ok(Some(condition))
    }
}
