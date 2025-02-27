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

use std::fmt::Debug;
use std::marker::PhantomData;
use std::ops::Range;

use common_arrow::arrow::buffer::Buffer;
use enum_as_inner::EnumAsInner;
use itertools::Itertools;
use num_traits::NumCast;
use ordered_float::OrderedFloat;
use serde::Deserialize;
use serde::Serialize;

use crate::property::Domain;
use crate::types::ArgType;
use crate::types::DataType;
use crate::types::GenericMap;
use crate::types::ValueType;
use crate::util::buffer_into_mut;
use crate::values::Column;
use crate::values::Scalar;
use crate::ColumnBuilder;
use crate::ScalarRef;

pub type F32 = OrderedFloat<f32>;
pub type F64 = OrderedFloat<f64>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberType<T: Number>(PhantomData<T>);

pub type Int8Type = NumberType<i8>;
pub type Int16Type = NumberType<i16>;
pub type Int32Type = NumberType<i32>;
pub type Int64Type = NumberType<i64>;
pub type UInt8Type = NumberType<u8>;
pub type UInt16Type = NumberType<u16>;
pub type UInt32Type = NumberType<u32>;
pub type UInt64Type = NumberType<u64>;
pub type Float32Type = NumberType<F32>;
pub type Float64Type = NumberType<F64>;

impl<Num: Number> ValueType for NumberType<Num> {
    type Scalar = Num;
    type ScalarRef<'a> = Num;
    type Column = Buffer<Num>;
    type Domain = SimpleDomain<Num>;
    type ColumnIterator<'a> = std::iter::Cloned<std::slice::Iter<'a, Num>>;
    type ColumnBuilder = Vec<Num>;

    #[inline]
    fn upcast_gat<'short, 'long: 'short>(long: Num) -> Num {
        long
    }

    fn to_owned_scalar<'a>(scalar: Self::ScalarRef<'a>) -> Self::Scalar {
        scalar
    }

    fn to_scalar_ref<'a>(scalar: &'a Self::Scalar) -> Self::ScalarRef<'a> {
        *scalar
    }

    fn try_downcast_scalar<'a>(scalar: &'a ScalarRef) -> Option<Self::ScalarRef<'a>> {
        Num::try_downcast_scalar(scalar.as_number()?)
    }

    fn try_downcast_column<'a>(col: &'a Column) -> Option<Self::Column> {
        Num::try_downcast_column(col.as_number()?)
    }

    fn try_downcast_domain(domain: &Domain) -> Option<SimpleDomain<Num>> {
        Num::try_downcast_domain(domain.as_number()?)
    }

    fn try_downcast_builder<'a>(
        builder: &'a mut ColumnBuilder,
    ) -> Option<&'a mut Self::ColumnBuilder> {
        match builder {
            ColumnBuilder::Number(num) => Num::try_downcast_builder(num),
            _ => None,
        }
    }

    fn upcast_scalar(scalar: Self::Scalar) -> Scalar {
        Scalar::Number(Num::upcast_scalar(scalar))
    }

    fn upcast_column(col: Self::Column) -> Column {
        Column::Number(Num::upcast_column(col))
    }

    fn upcast_domain(domain: SimpleDomain<Num>) -> Domain {
        Domain::Number(Num::upcast_domain(domain))
    }

    fn column_len<'a>(col: &'a Self::Column) -> usize {
        col.len()
    }

    fn index_column<'a>(col: &'a Self::Column, index: usize) -> Option<Self::ScalarRef<'a>> {
        col.get(index).cloned()
    }

    unsafe fn index_column_unchecked<'a>(
        col: &'a Self::Column,
        index: usize,
    ) -> Self::ScalarRef<'a> {
        *col.get_unchecked(index)
    }

    fn slice_column<'a>(col: &'a Self::Column, range: Range<usize>) -> Self::Column {
        col.clone().slice(range.start, range.end - range.start)
    }

    fn iter_column<'a>(col: &'a Self::Column) -> Self::ColumnIterator<'a> {
        col.iter().cloned()
    }

    fn column_to_builder(col: Self::Column) -> Self::ColumnBuilder {
        buffer_into_mut(col)
    }

    fn builder_len(builder: &Self::ColumnBuilder) -> usize {
        builder.len()
    }

    fn push_item(builder: &mut Self::ColumnBuilder, item: Self::Scalar) {
        builder.push(item);
    }

    fn push_default(builder: &mut Self::ColumnBuilder) {
        builder.push(Num::default());
    }

    fn append_builder(builder: &mut Self::ColumnBuilder, other: &Self::ColumnBuilder) {
        builder.extend_from_slice(other);
    }

    fn build_column(builder: Self::ColumnBuilder) -> Self::Column {
        builder.into()
    }

    fn build_scalar(builder: Self::ColumnBuilder) -> Self::Scalar {
        assert_eq!(builder.len(), 1);
        builder[0]
    }
}

impl<Num: Number> ArgType for NumberType<Num> {
    fn data_type() -> DataType {
        DataType::Number(Num::data_type())
    }

    fn create_builder(capacity: usize, _generics: &GenericMap) -> Self::ColumnBuilder {
        Vec::with_capacity(capacity)
    }

    fn column_from_vec(vec: Vec<Self::Scalar>, _generics: &GenericMap) -> Self::Column {
        vec.into()
    }

    fn column_from_iter(iter: impl Iterator<Item = Self::Scalar>, _: &GenericMap) -> Self::Column {
        iter.collect()
    }

    fn column_from_ref_iter<'a>(
        iter: impl Iterator<Item = Self::ScalarRef<'a>>,
        _: &GenericMap,
    ) -> Self::Column {
        iter.collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumAsInner)]
pub enum NumberDataType {
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Int8,
    Int16,
    Int32,
    Int64,
    Float32,
    Float64,
}

#[derive(Clone, Copy, PartialEq, Eq, EnumAsInner, Serialize, Deserialize)]
pub enum NumberScalar {
    UInt8(u8),
    UInt16(u16),
    UInt32(u32),
    UInt64(u64),
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    Float32(F32),
    Float64(F64),
}

#[derive(Clone, PartialEq, EnumAsInner)]
pub enum NumberColumn {
    UInt8(Buffer<u8>),
    UInt16(Buffer<u16>),
    UInt32(Buffer<u32>),
    UInt64(Buffer<u64>),
    Int8(Buffer<i8>),
    Int16(Buffer<i16>),
    Int32(Buffer<i32>),
    Int64(Buffer<i64>),
    Float32(Buffer<F32>),
    Float64(Buffer<F64>),
}

#[derive(Debug, Clone, PartialEq, Eq, EnumAsInner)]
pub enum NumberColumnBuilder {
    UInt8(Vec<u8>),
    UInt16(Vec<u16>),
    UInt32(Vec<u32>),
    UInt64(Vec<u64>),
    Int8(Vec<i8>),
    Int16(Vec<i16>),
    Int32(Vec<i32>),
    Int64(Vec<i64>),
    Float32(Vec<F32>),
    Float64(Vec<F64>),
}

#[derive(Debug, Clone, PartialEq, Eq, EnumAsInner)]
pub enum NumberDomain {
    UInt8(SimpleDomain<u8>),
    UInt16(SimpleDomain<u16>),
    UInt32(SimpleDomain<u32>),
    UInt64(SimpleDomain<u64>),
    Int8(SimpleDomain<i8>),
    Int16(SimpleDomain<i16>),
    Int32(SimpleDomain<i32>),
    Int64(SimpleDomain<i64>),
    Float32(SimpleDomain<F32>),
    Float64(SimpleDomain<F64>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleDomain<T> {
    pub min: T,
    pub max: T,
}

impl NumberDataType {
    pub const fn new(bit_width: u8, is_signed: bool, is_float: bool) -> Self {
        match (bit_width, is_signed, is_float) {
            (8, false, false) => NumberDataType::UInt8,
            (16, false, false) => NumberDataType::UInt16,
            (32, false, false) => NumberDataType::UInt32,
            (64, false, false) => NumberDataType::UInt64,
            (8, true, false) => NumberDataType::Int8,
            (16, true, false) => NumberDataType::Int16,
            (32, true, false) => NumberDataType::Int32,
            (64, true, false) => NumberDataType::Int64,
            (32, true, true) => NumberDataType::Float32,
            (64, true, true) => NumberDataType::Float64,
            _ => panic!("unsupported numeric type"),
        }
    }

    pub const fn bit_width(&self) -> u8 {
        match self {
            NumberDataType::UInt8 => 8,
            NumberDataType::UInt16 => 16,
            NumberDataType::UInt32 => 32,
            NumberDataType::UInt64 => 64,
            NumberDataType::Int8 => 8,
            NumberDataType::Int16 => 16,
            NumberDataType::Int32 => 32,
            NumberDataType::Int64 => 64,
            NumberDataType::Float32 => 32,
            NumberDataType::Float64 => 64,
        }
    }

    pub const fn is_signed(&self) -> bool {
        match self {
            NumberDataType::UInt8 => false,
            NumberDataType::UInt16 => false,
            NumberDataType::UInt32 => false,
            NumberDataType::UInt64 => false,
            NumberDataType::Int8 => true,
            NumberDataType::Int16 => true,
            NumberDataType::Int32 => true,
            NumberDataType::Int64 => true,
            NumberDataType::Float32 => true,
            NumberDataType::Float64 => true,
        }
    }

    pub const fn is_float(&self) -> bool {
        match self {
            NumberDataType::UInt8 => false,
            NumberDataType::UInt16 => false,
            NumberDataType::UInt32 => false,
            NumberDataType::UInt64 => false,
            NumberDataType::Int8 => false,
            NumberDataType::Int16 => false,
            NumberDataType::Int32 => false,
            NumberDataType::Int64 => false,
            NumberDataType::Float32 => true,
            NumberDataType::Float64 => true,
        }
    }

    pub const fn can_lossless_cast_to(self, dest: Self) -> bool {
        match (self.is_float(), dest.is_float()) {
            (true, true) => self.bit_width() <= dest.bit_width(),
            (true, false) => false,
            (false, true) => self.bit_width() < dest.bit_width(),
            (false, false) => match (self.is_signed(), dest.is_signed()) {
                (true, true) | (false, false) => self.bit_width() <= dest.bit_width(),
                (false, true) => {
                    if let Some(self_next_bit_width) = next_bit_width(self.bit_width()) {
                        self_next_bit_width <= dest.bit_width()
                    } else {
                        false
                    }
                }
                (true, false) => false,
            },
        }
    }

    pub const fn lossless_super_type(self, other: Self) -> Option<Self> {
        Some(match (self.is_float(), other.is_float()) {
            (true, true) => NumberDataType::new(
                max_bit_with(self.bit_width(), other.bit_width()),
                true,
                true,
            ),
            (true, false) => {
                let bin_width =
                    if let Some(next_other_bit_width) = next_bit_width(other.bit_width()) {
                        max_bit_with(self.bit_width(), next_other_bit_width)
                    } else {
                        return None;
                    };
                NumberDataType::new(bin_width, true, true)
            }
            (false, true) => {
                let bin_width = if let Some(next_self_bit_width) = next_bit_width(self.bit_width())
                {
                    max_bit_with(next_self_bit_width, other.bit_width())
                } else {
                    return None;
                };
                NumberDataType::new(bin_width, true, true)
            }
            (false, false) => match (self.is_signed(), other.is_signed()) {
                (true, true) => NumberDataType::new(
                    max_bit_with(self.bit_width(), other.bit_width()),
                    true,
                    false,
                ),
                (false, false) => NumberDataType::new(
                    max_bit_with(self.bit_width(), other.bit_width()),
                    false,
                    false,
                ),
                (false, true) => {
                    let bin_width =
                        if let Some(next_other_bit_width) = next_bit_width(other.bit_width()) {
                            max_bit_with(self.bit_width(), next_other_bit_width)
                        } else {
                            return None;
                        };
                    NumberDataType::new(bin_width, true, false)
                }
                (true, false) => {
                    let bin_width =
                        if let Some(next_self_bit_width) = next_bit_width(self.bit_width()) {
                            max_bit_with(next_self_bit_width, other.bit_width())
                        } else {
                            return None;
                        };
                    NumberDataType::new(bin_width, true, false)
                }
            },
        })
    }
}

const fn next_bit_width(width: u8) -> Option<u8> {
    match width {
        8 => Some(16),
        16 => Some(32),
        32 => Some(64),
        64 => None,
        _ => panic!("invalid bit width"),
    }
}

const fn max_bit_with(lhs: u8, rhs: u8) -> u8 {
    if lhs > rhs { lhs } else { rhs }
}

impl PartialOrd for NumberScalar {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        crate::with_number_type!(|NUM_TYPE| match (self, other) {
            (NumberScalar::NUM_TYPE(lhs), NumberScalar::NUM_TYPE(rhs)) => lhs.partial_cmp(rhs),
            _ => None,
        })
    }
}

impl NumberScalar {
    pub fn domain(&self) -> NumberDomain {
        crate::with_number_type!(|NUM_TYPE| match self {
            NumberScalar::NUM_TYPE(num) => NumberDomain::NUM_TYPE(SimpleDomain {
                min: *num,
                max: *num,
            }),
        })
    }
}

impl NumberColumn {
    pub fn len(&self) -> usize {
        crate::with_number_type!(|NUM_TYPE| match self {
            NumberColumn::NUM_TYPE(col) => col.len(),
        })
    }

    pub fn index(&self, index: usize) -> Option<NumberScalar> {
        crate::with_number_type!(|NUM_TYPE| match self {
            NumberColumn::NUM_TYPE(col) => Some(NumberScalar::NUM_TYPE(col.get(index).cloned()?)),
        })
    }

    /// # Safety
    /// Assumes that the `index` is not out of range.
    pub unsafe fn index_unchecked(&self, index: usize) -> NumberScalar {
        crate::with_number_type!(|NUM_TYPE| match self {
            NumberColumn::NUM_TYPE(col) => NumberScalar::NUM_TYPE(*col.get_unchecked(index)),
        })
    }

    pub fn slice(&self, range: Range<usize>) -> Self {
        assert!(
            range.end <= self.len(),
            "range {:?} out of len {}",
            range,
            self.len()
        );

        crate::with_number_type!(|NUM_TYPE| match self {
            NumberColumn::NUM_TYPE(col) => {
                NumberColumn::NUM_TYPE(col.clone().slice(range.start, range.end - range.start))
            }
        })
    }

    pub fn domain(&self) -> NumberDomain {
        assert!(self.len() > 0);
        crate::with_number_type!(|NUM_TYPE| match self {
            NumberColumn::NUM_TYPE(col) => {
                let (min, max) = col.iter().minmax().into_option().unwrap();
                NumberDomain::NUM_TYPE(SimpleDomain {
                    min: *min,
                    max: *max,
                })
            }
        })
    }
}

impl NumberColumnBuilder {
    pub fn from_column(col: NumberColumn) -> Self {
        crate::with_number_type!(|NUM_TYPE| match col {
            NumberColumn::NUM_TYPE(col) => NumberColumnBuilder::NUM_TYPE(buffer_into_mut(col)),
        })
    }

    pub fn repeat(scalar: NumberScalar, n: usize) -> NumberColumnBuilder {
        crate::with_number_type!(|NUM_TYPE| match scalar {
            NumberScalar::NUM_TYPE(num) => NumberColumnBuilder::NUM_TYPE(vec![num; n]),
        })
    }

    pub fn len(&self) -> usize {
        crate::with_number_type!(|NUM_TYPE| match self {
            NumberColumnBuilder::NUM_TYPE(col) => col.len(),
        })
    }

    pub fn with_capacity(ty: &NumberDataType, capacity: usize) -> Self {
        crate::with_number_type!(|NUM_TYPE| match ty {
            NumberDataType::NUM_TYPE => NumberColumnBuilder::NUM_TYPE(Vec::with_capacity(capacity)),
        })
    }

    pub fn push(&mut self, item: NumberScalar) {
        crate::with_number_type!(|NUM_TYPE| match (self, item) {
            (NumberColumnBuilder::NUM_TYPE(builder), NumberScalar::NUM_TYPE(value)) => {
                builder.push(value)
            }
            (builder, scalar) => unreachable!("unable to push {scalar:?} to {builder:?}"),
        })
    }

    pub fn push_default(&mut self) {
        crate::with_number_mapped_type!(|NUM_TYPE| match self {
            NumberColumnBuilder::NUM_TYPE(builder) => builder.push(NUM_TYPE::default()),
        })
    }

    pub fn append(&mut self, other: &NumberColumnBuilder) {
        crate::with_number_type!(|NUM_TYPE| match (self, other) {
            (
                NumberColumnBuilder::NUM_TYPE(builder),
                NumberColumnBuilder::NUM_TYPE(other_builder),
            ) => {
                builder.extend_from_slice(other_builder);
            }
            (this, other) => unreachable!("unable append {other:?} onto {this:?}"),
        })
    }

    pub fn build(self) -> NumberColumn {
        crate::with_number_type!(|NUM_TYPE| match self {
            NumberColumnBuilder::NUM_TYPE(builder) => NumberColumn::NUM_TYPE(builder.into()),
        })
    }

    pub fn build_scalar(self) -> NumberScalar {
        assert_eq!(self.len(), 1);

        crate::with_number_type!(|NUM_TYPE| match self {
            NumberColumnBuilder::NUM_TYPE(builder) => NumberScalar::NUM_TYPE(builder[0]),
        })
    }
}

impl<T: Number> SimpleDomain<T> {
    /// Returns the saturating cast domain and a flag denoting whether overflow happened.
    pub fn overflow_cast<U: Number>(&self) -> (SimpleDomain<U>, bool) {
        self.overflow_cast_with_minmax(U::MIN, U::MAX)
    }

    pub fn overflow_cast_with_minmax<U: Number>(&self, min: U, max: U) -> (SimpleDomain<U>, bool) {
        let (min, min_overflowing) = overflow_cast_with_minmax::<T, U>(self.min, min, max);
        let (max, max_overflowing) = overflow_cast_with_minmax::<T, U>(self.max, min, max);
        (
            SimpleDomain { min, max },
            min_overflowing || max_overflowing,
        )
    }
}

fn overflow_cast_with_minmax<T: Number, U: Number>(src: T, min: U, max: U) -> (U, bool) {
    let dest_min: T = num_traits::cast(min).unwrap_or(T::MIN);
    let dest_max: T = num_traits::cast(max).unwrap_or(T::MAX);
    let src_clamp: T = src.clamp(dest_min, dest_max);
    let overflowing = src != src_clamp;
    // The number must be within the range that `U` can represent after clamping, therefore
    // it's safe to unwrap.
    let dest: U = num_traits::cast(src_clamp).unwrap();

    (dest, overflowing)
}

#[macro_export]
macro_rules! with_number_type {
    ( | $t:tt | $($tail:tt)* ) => {
        match_template::match_template! {
            $t = [UInt8, UInt16, UInt32, UInt64, Int8, Int16, Int32, Int64, Float32, Float64],
            $($tail)*
        }
    }
}

#[macro_export]
macro_rules! with_unsigned_number_mapped_type {
    (| $t:tt | $($tail:tt)*) => {
        match_template::match_template! {
            $t = [
                UInt8 => u8, UInt16 => u16, UInt32 => u32, UInt64 => u64
            ],
            $($tail)*
        }
    }
}

#[macro_export]
macro_rules! with_number_mapped_type {
    (| $t:tt | $($tail:tt)*) => {
        match_template::match_template! {
            $t = [
                UInt8 => u8, UInt16 => u16, UInt32 => u32, UInt64 => u64,
                Int8 => i8, Int16 => i16, Int32 => i32, Int64 => i64,
                Float32 => $crate::types::number::F32, Float64 => $crate::types::number::F64
            ],
            $($tail)*
        }
    }
}

#[macro_export]
macro_rules! with_number_data_types {
    (
    $type0:expr, $type1:expr, | $_a:tt $T0:ident, $_b:tt $T1:ident | $body:tt,  $nbody:tt
) => {{
        use common_expression::types::number::NumberDataType::*;
        use common_expression::types::number::F32;
        use common_expression::types::number::F64;

        macro_rules! __with_types__ {
            ( $_a $T0:ident, $_b $T1:ident ) => {
                $body
            };
        }

        macro_rules! __match_type__ {
            ($t:ident) => {
                match $type1 {
                    Int8 => __with_types__! { $t, i8 },
                    Int16 => __with_types__! { $t, i16 },
                    Int32 => __with_types__! { $t, i32 },
                    Int64 => __with_types__! { $t, i64 },
                    UInt8 => __with_types__! { $t, u8 },
                    UInt16 => __with_types__! { $t, u16 },
                    UInt32 => __with_types__! { $t, u32 },
                    UInt64 => __with_types__! { $t, u64 },
                    Float32 => __with_types__! { $t, F32 },
                    Float64 => __with_types__! { $t, F64 },
                    _ => $nbody,
                }
            };
        }

        match $type0 {
            Int8 => __match_type__! { i8 },
            Int16 => __match_type__! { i16 },
            Int32 => __match_type__! { i32 },
            Int64 => __match_type__! { i64 },
            UInt8 => __match_type__! { u8 },
            UInt16 => __match_type__! { u16 },
            UInt32 => __match_type__! { u32 },
            UInt64 => __match_type__! { u64 },
            Float32 => __match_type__! { F32 },
            Float64 => __match_type__! { F64 },
            _ => $nbody,
        }
    }};
}

pub trait Number:
    Copy
    + Debug
    + NumCast
    + Default
    + Clone
    + Copy
    + PartialEq
    + Eq
    + PartialOrd
    + Ord
    + Sync
    + Send
    + 'static
{
    const MIN: Self;
    const MAX: Self;

    fn data_type() -> NumberDataType;
    fn try_downcast_scalar(scalar: &NumberScalar) -> Option<Self>;
    fn try_downcast_column(col: &NumberColumn) -> Option<Buffer<Self>>;
    // TODO
    fn try_downcast_builder(col: &mut NumberColumnBuilder) -> Option<&mut Vec<Self>>;
    fn try_downcast_domain(domain: &NumberDomain) -> Option<SimpleDomain<Self>>;
    fn upcast_scalar(scalar: Self) -> NumberScalar;
    fn upcast_column(col: Buffer<Self>) -> NumberColumn;
    fn upcast_domain(domain: SimpleDomain<Self>) -> NumberDomain;
}

impl Number for u8 {
    const MIN: Self = u8::MIN;
    const MAX: Self = u8::MAX;

    fn data_type() -> NumberDataType {
        NumberDataType::UInt8
    }

    fn try_downcast_scalar(scalar: &NumberScalar) -> Option<Self> {
        scalar.as_u_int8().cloned()
    }

    fn try_downcast_column(col: &NumberColumn) -> Option<Buffer<Self>> {
        col.as_u_int8().cloned()
    }

    fn try_downcast_builder(builder: &mut NumberColumnBuilder) -> Option<&mut Vec<Self>> {
        builder.as_u_int8_mut()
    }

    fn try_downcast_domain(domain: &NumberDomain) -> Option<SimpleDomain<Self>> {
        domain.as_u_int8().cloned()
    }

    fn upcast_scalar(scalar: Self) -> NumberScalar {
        NumberScalar::UInt8(scalar)
    }

    fn upcast_column(col: Buffer<Self>) -> NumberColumn {
        NumberColumn::UInt8(col)
    }

    fn upcast_domain(domain: SimpleDomain<Self>) -> NumberDomain {
        NumberDomain::UInt8(domain)
    }
}

impl Number for u16 {
    const MIN: Self = u16::MIN;
    const MAX: Self = u16::MAX;

    fn data_type() -> NumberDataType {
        NumberDataType::UInt16
    }

    fn try_downcast_scalar(scalar: &NumberScalar) -> Option<Self> {
        scalar.as_u_int16().cloned()
    }

    fn try_downcast_column(col: &NumberColumn) -> Option<Buffer<Self>> {
        col.as_u_int16().cloned()
    }

    fn try_downcast_builder(builder: &mut NumberColumnBuilder) -> Option<&mut Vec<Self>> {
        builder.as_u_int16_mut()
    }

    fn try_downcast_domain(domain: &NumberDomain) -> Option<SimpleDomain<Self>> {
        domain.as_u_int16().cloned()
    }

    fn upcast_scalar(scalar: Self) -> NumberScalar {
        NumberScalar::UInt16(scalar)
    }

    fn upcast_column(col: Buffer<Self>) -> NumberColumn {
        NumberColumn::UInt16(col)
    }

    fn upcast_domain(domain: SimpleDomain<Self>) -> NumberDomain {
        NumberDomain::UInt16(domain)
    }
}

impl Number for u32 {
    const MIN: Self = u32::MIN;
    const MAX: Self = u32::MAX;

    fn data_type() -> NumberDataType {
        NumberDataType::UInt32
    }

    fn try_downcast_scalar(scalar: &NumberScalar) -> Option<Self> {
        scalar.as_u_int32().cloned()
    }

    fn try_downcast_column(col: &NumberColumn) -> Option<Buffer<Self>> {
        col.as_u_int32().cloned()
    }

    fn try_downcast_builder(builder: &mut NumberColumnBuilder) -> Option<&mut Vec<Self>> {
        builder.as_u_int32_mut()
    }

    fn try_downcast_domain(domain: &NumberDomain) -> Option<SimpleDomain<Self>> {
        domain.as_u_int32().cloned()
    }

    fn upcast_scalar(scalar: Self) -> NumberScalar {
        NumberScalar::UInt32(scalar)
    }

    fn upcast_column(col: Buffer<Self>) -> NumberColumn {
        NumberColumn::UInt32(col)
    }

    fn upcast_domain(domain: SimpleDomain<Self>) -> NumberDomain {
        NumberDomain::UInt32(domain)
    }
}

impl Number for u64 {
    const MIN: Self = u64::MIN;
    const MAX: Self = u64::MAX;

    fn data_type() -> NumberDataType {
        NumberDataType::UInt64
    }

    fn try_downcast_scalar(scalar: &NumberScalar) -> Option<Self> {
        scalar.as_u_int64().cloned()
    }

    fn try_downcast_column(col: &NumberColumn) -> Option<Buffer<Self>> {
        col.as_u_int64().cloned()
    }

    fn try_downcast_builder(builder: &mut NumberColumnBuilder) -> Option<&mut Vec<Self>> {
        builder.as_u_int64_mut()
    }

    fn try_downcast_domain(domain: &NumberDomain) -> Option<SimpleDomain<Self>> {
        domain.as_u_int64().cloned()
    }

    fn upcast_scalar(scalar: Self) -> NumberScalar {
        NumberScalar::UInt64(scalar)
    }

    fn upcast_column(col: Buffer<Self>) -> NumberColumn {
        NumberColumn::UInt64(col)
    }

    fn upcast_domain(domain: SimpleDomain<Self>) -> NumberDomain {
        NumberDomain::UInt64(domain)
    }
}

impl Number for i8 {
    const MIN: Self = i8::MIN;
    const MAX: Self = i8::MAX;

    fn data_type() -> NumberDataType {
        NumberDataType::Int8
    }

    fn try_downcast_scalar(scalar: &NumberScalar) -> Option<Self> {
        scalar.as_int8().cloned()
    }

    fn try_downcast_column(col: &NumberColumn) -> Option<Buffer<Self>> {
        col.as_int8().cloned()
    }

    fn try_downcast_builder(builder: &mut NumberColumnBuilder) -> Option<&mut Vec<Self>> {
        builder.as_int8_mut()
    }

    fn try_downcast_domain(domain: &NumberDomain) -> Option<SimpleDomain<Self>> {
        domain.as_int8().cloned()
    }

    fn upcast_scalar(scalar: Self) -> NumberScalar {
        NumberScalar::Int8(scalar)
    }

    fn upcast_column(col: Buffer<Self>) -> NumberColumn {
        NumberColumn::Int8(col)
    }

    fn upcast_domain(domain: SimpleDomain<Self>) -> NumberDomain {
        NumberDomain::Int8(domain)
    }
}

impl Number for i16 {
    const MIN: Self = i16::MIN;
    const MAX: Self = i16::MAX;

    fn data_type() -> NumberDataType {
        NumberDataType::Int16
    }

    fn try_downcast_scalar(scalar: &NumberScalar) -> Option<Self> {
        scalar.as_int16().cloned()
    }

    fn try_downcast_column(col: &NumberColumn) -> Option<Buffer<Self>> {
        col.as_int16().cloned()
    }

    fn try_downcast_builder(builder: &mut NumberColumnBuilder) -> Option<&mut Vec<Self>> {
        builder.as_int16_mut()
    }

    fn try_downcast_domain(domain: &NumberDomain) -> Option<SimpleDomain<Self>> {
        domain.as_int16().cloned()
    }

    fn upcast_scalar(scalar: Self) -> NumberScalar {
        NumberScalar::Int16(scalar)
    }

    fn upcast_column(col: Buffer<Self>) -> NumberColumn {
        NumberColumn::Int16(col)
    }

    fn upcast_domain(domain: SimpleDomain<Self>) -> NumberDomain {
        NumberDomain::Int16(domain)
    }
}

impl Number for i32 {
    const MIN: Self = i32::MIN;
    const MAX: Self = i32::MAX;

    fn data_type() -> NumberDataType {
        NumberDataType::Int32
    }

    fn try_downcast_scalar(scalar: &NumberScalar) -> Option<Self> {
        scalar.as_int32().cloned()
    }

    fn try_downcast_column(col: &NumberColumn) -> Option<Buffer<Self>> {
        col.as_int32().cloned()
    }

    fn try_downcast_builder(builder: &mut NumberColumnBuilder) -> Option<&mut Vec<Self>> {
        builder.as_int32_mut()
    }

    fn try_downcast_domain(domain: &NumberDomain) -> Option<SimpleDomain<Self>> {
        domain.as_int32().cloned()
    }

    fn upcast_scalar(scalar: Self) -> NumberScalar {
        NumberScalar::Int32(scalar)
    }

    fn upcast_column(col: Buffer<Self>) -> NumberColumn {
        NumberColumn::Int32(col)
    }

    fn upcast_domain(domain: SimpleDomain<Self>) -> NumberDomain {
        NumberDomain::Int32(domain)
    }
}

impl Number for i64 {
    const MIN: Self = i64::MIN;
    const MAX: Self = i64::MAX;

    fn data_type() -> NumberDataType {
        NumberDataType::Int64
    }

    fn try_downcast_scalar(scalar: &NumberScalar) -> Option<Self> {
        scalar.as_int64().cloned()
    }

    fn try_downcast_column(col: &NumberColumn) -> Option<Buffer<Self>> {
        col.as_int64().cloned()
    }

    fn try_downcast_builder(builder: &mut NumberColumnBuilder) -> Option<&mut Vec<Self>> {
        builder.as_int64_mut()
    }

    fn try_downcast_domain(domain: &NumberDomain) -> Option<SimpleDomain<Self>> {
        domain.as_int64().cloned()
    }

    fn upcast_scalar(scalar: Self) -> NumberScalar {
        NumberScalar::Int64(scalar)
    }

    fn upcast_column(col: Buffer<Self>) -> NumberColumn {
        NumberColumn::Int64(col)
    }

    fn upcast_domain(domain: SimpleDomain<Self>) -> NumberDomain {
        NumberDomain::Int64(domain)
    }
}

impl Number for F32 {
    const MIN: Self = OrderedFloat(f32::NEG_INFINITY);
    const MAX: Self = OrderedFloat(f32::NAN);

    fn data_type() -> NumberDataType {
        NumberDataType::Float32
    }

    fn try_downcast_scalar(scalar: &NumberScalar) -> Option<Self> {
        scalar.as_float32().cloned()
    }

    fn try_downcast_column(col: &NumberColumn) -> Option<Buffer<Self>> {
        col.as_float32().cloned()
    }

    fn try_downcast_builder(builder: &mut NumberColumnBuilder) -> Option<&mut Vec<Self>> {
        builder.as_float32_mut()
    }

    fn try_downcast_domain(domain: &NumberDomain) -> Option<SimpleDomain<Self>> {
        domain.as_float32().cloned()
    }

    fn upcast_scalar(scalar: Self) -> NumberScalar {
        NumberScalar::Float32(scalar)
    }

    fn upcast_column(col: Buffer<Self>) -> NumberColumn {
        NumberColumn::Float32(col)
    }

    fn upcast_domain(domain: SimpleDomain<Self>) -> NumberDomain {
        NumberDomain::Float32(domain)
    }
}

impl Number for F64 {
    const MIN: Self = OrderedFloat(f64::NEG_INFINITY);
    const MAX: Self = OrderedFloat(f64::NAN);

    fn data_type() -> NumberDataType {
        NumberDataType::Float64
    }

    fn try_downcast_scalar(scalar: &NumberScalar) -> Option<Self> {
        scalar.as_float64().cloned()
    }

    fn try_downcast_column(col: &NumberColumn) -> Option<Buffer<Self>> {
        col.as_float64().cloned()
    }

    fn try_downcast_builder(builder: &mut NumberColumnBuilder) -> Option<&mut Vec<Self>> {
        builder.as_float64_mut()
    }

    fn try_downcast_domain(domain: &NumberDomain) -> Option<SimpleDomain<Self>> {
        domain.as_float64().cloned()
    }

    fn upcast_scalar(scalar: Self) -> NumberScalar {
        NumberScalar::Float64(scalar)
    }

    fn upcast_column(col: Buffer<Self>) -> NumberColumn {
        NumberColumn::Float64(col)
    }

    fn upcast_domain(domain: SimpleDomain<Self>) -> NumberDomain {
        NumberDomain::Float64(domain)
    }
}
