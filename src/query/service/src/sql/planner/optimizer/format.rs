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

use std::collections::HashMap;

use common_ast::ast::FormatTreeNode;
use common_exception::ErrorCode;
use common_exception::Result;
use common_planner::IndexType;

use super::cost::CostContext;
use crate::sql::optimizer::group::Group;
use crate::sql::optimizer::MExpr;
use crate::sql::optimizer::Memo;
use crate::sql::plans::RelOperator;

pub fn display_memo(memo: &Memo, cost_map: &HashMap<IndexType, CostContext>) -> Result<String> {
    Ok(memo
        .groups
        .iter()
        .map(|grp| {
            group_to_format_tree(
                grp,
                cost_map.get(&grp.group_index).ok_or_else(|| {
                    ErrorCode::LogicalError(format!(
                        "Cannot find cost context for group {}",
                        grp.group_index
                    ))
                })?,
            )
            .format_pretty()
        })
        .collect::<Result<Vec<_>>>()?
        .join("\n"))
}

pub fn display_rel_op(rel_op: &RelOperator) -> String {
    match rel_op {
        RelOperator::LogicalGet(_) => "LogicalGet".to_string(),
        RelOperator::LogicalInnerJoin(_) => "LogicalInnerJoin".to_string(),
        RelOperator::PhysicalScan(_) => "PhysicalScan".to_string(),
        RelOperator::PhysicalHashJoin(_) => "PhysicalHashJoin".to_string(),
        RelOperator::EvalScalar(_) => "EvalScalar".to_string(),
        RelOperator::Filter(_) => "Filter".to_string(),
        RelOperator::Aggregate(_) => "Aggregate".to_string(),
        RelOperator::Sort(_) => "Sort".to_string(),
        RelOperator::Limit(_) => "Limit".to_string(),
        RelOperator::UnionAll(_) => "UnionAll".to_string(),
        RelOperator::Exchange(_) => "Exchange".to_string(),
        RelOperator::Pattern(_) => "Pattern".to_string(),
        RelOperator::DummyTableScan(_) => "DummyTableScan".to_string(),
    }
}

fn group_to_format_tree(group: &Group, cost_context: &CostContext) -> FormatTreeNode<String> {
    FormatTreeNode::with_children(
        format!("Group #{}", group.group_index),
        vec![
            vec![FormatTreeNode::new(format!(
                "best cost: [#{}] {}",
                cost_context.expr_index, cost_context.cost
            ))],
            group.m_exprs.iter().map(m_expr_to_format_tree).collect(),
        ]
        .concat(),
    )
}

fn m_expr_to_format_tree(m_expr: &MExpr) -> FormatTreeNode<String> {
    FormatTreeNode::new(format!(
        "{} [{}]",
        display_rel_op(&m_expr.plan),
        m_expr
            .children
            .iter()
            .map(|child| format!("#{child}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}
