// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbNifMath.pas

//! Vectors, quaternions, matrices, transforms and the geometry helpers of
//! the NIF code: rotations between their forms, strips and triangles,
//! bounds, face normals and tangent spaces.
//!
//! `StripifyTriangles` needs `meshopt_stripify` of `wbMeshOptimize`, which
//! is ported with LOD generation (phase 5 step 6).

use std::ops::{Add, Div, Mul, Sub};

use xedit_core::delphi::{MAX_SINGLE, same_value};

use crate::data_format::{DfError, R};

/// `TMatrix33`.
pub type Matrix33 = [[f64; 3]; 3];

/// `TQuaternion`: `q = [w, x, y, z]`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Quaternion {
    pub q: [f64; 4],
}

impl Quaternion {
    pub fn w(&self) -> f64 {
        self.q[0]
    }
    pub fn x(&self) -> f64 {
        self.q[1]
    }
    pub fn y(&self) -> f64 {
        self.q[2]
    }
    pub fn z(&self) -> f64 {
        self.q[3]
    }

    pub fn new(w: f64, x: f64, y: f64, z: f64) -> Quaternion {
        Quaternion { q: [w, x, y, z] }
    }

    /// `SetIdentity`.
    pub fn set_identity(&mut self) {
        self.q = [1.0, 0.0, 0.0, 0.0];
    }

    /// `IsIdentity`.
    pub fn is_identity(&self) -> bool {
        same_value(self.w(), 1.0) && same_value(self.x(), 0.0) && same_value(self.y(), 0.0) && same_value(self.z(), 0.0)
    }
}

impl Mul for Quaternion {
    type Output = Quaternion;
    fn mul(self, b: Quaternion) -> Quaternion {
        let a = self;
        Quaternion::new(
            a.w() * b.w() - a.x() * b.x() - a.y() * b.y() - a.z() * b.z(),
            a.w() * b.x() + a.x() * b.w() + a.y() * b.z() - a.z() * b.y(),
            a.w() * b.y() - a.x() * b.z() + a.y() * b.w() + a.z() * b.x(),
            a.w() * b.z() + a.x() * b.y() - a.y() * b.x() + a.z() * b.w(),
        )
    }
}

/// `TVector2`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector2 {
    pub v: [f64; 2],
}

impl Vector2 {
    pub fn new(x: f64, y: f64) -> Vector2 {
        Vector2 { v: [x, y] }
    }

    /// `ValidateNan`.
    pub fn validate_nan(&mut self) {
        for value in &mut self.v {
            if value.is_nan() {
                *value = 0.0;
            }
        }
    }
}

impl Add for Vector2 {
    type Output = Vector2;
    fn add(self, b: Vector2) -> Vector2 {
        Vector2::new(self.v[0] + b.v[0], self.v[1] + b.v[1])
    }
}

impl Sub for Vector2 {
    type Output = Vector2;
    fn sub(self, b: Vector2) -> Vector2 {
        Vector2::new(self.v[0] - b.v[0], self.v[1] - b.v[1])
    }
}

impl Mul for Vector2 {
    type Output = Vector2;
    fn mul(self, b: Vector2) -> Vector2 {
        Vector2::new(self.v[0] * b.v[0], self.v[1] * b.v[1])
    }
}

/// `TVector3`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vector3 {
    pub v: [f64; 3],
}

impl Vector3 {
    pub fn new(x: f64, y: f64, z: f64) -> Vector3 {
        Vector3 { v: [x, y, z] }
    }
    pub fn x(&self) -> f64 {
        self.v[0]
    }
    pub fn y(&self) -> f64 {
        self.v[1]
    }
    pub fn z(&self) -> f64 {
        self.v[2]
    }

    /// `IsZero`.
    pub fn is_zero(&self) -> bool {
        same_value(self.x(), 0.0) && same_value(self.y(), 0.0) && same_value(self.z(), 0.0)
    }

    /// `Length`.
    pub fn length(&self) -> f64 {
        (self.x() * self.x() + self.y() * self.y() + self.z() * self.z()).sqrt()
    }

    /// `Normalize`.
    pub fn normalize(&mut self) {
        let s = (self.x() * self.x() + self.y() * self.y() + self.z() * self.z()).sqrt();
        if s > 0.0 {
            for value in &mut self.v {
                *value /= s;
            }
        }
    }

    /// `Normalize(aLength)`.
    pub fn normalize_by(&mut self, length: f64) {
        if length > 0.0 {
            for value in &mut self.v {
                *value /= length;
            }
        }
    }

    /// `ValidateNan`.
    pub fn validate_nan(&mut self) {
        for value in &mut self.v {
            if value.is_nan() {
                *value = 0.0;
            }
        }
    }
}

impl Add for Vector3 {
    type Output = Vector3;
    fn add(self, b: Vector3) -> Vector3 {
        Vector3::new(self.x() + b.x(), self.y() + b.y(), self.z() + b.z())
    }
}

impl Sub for Vector3 {
    type Output = Vector3;
    fn sub(self, b: Vector3) -> Vector3 {
        Vector3::new(self.x() - b.x(), self.y() - b.y(), self.z() - b.z())
    }
}

impl Sub<f64> for Vector3 {
    type Output = Vector3;
    fn sub(self, b: f64) -> Vector3 {
        Vector3::new(self.x() - b, self.y() - b, self.z() - b)
    }
}

impl Mul for Vector3 {
    type Output = Vector3;
    fn mul(self, b: Vector3) -> Vector3 {
        Vector3::new(self.x() * b.x(), self.y() * b.y(), self.z() * b.z())
    }
}

impl Mul<f64> for Vector3 {
    type Output = Vector3;
    fn mul(self, b: f64) -> Vector3 {
        Vector3::new(self.x() * b, self.y() * b, self.z() * b)
    }
}

impl Mul<Quaternion> for Vector3 {
    type Output = Vector3;
    /// The vector rotated by the quaternion.
    fn mul(self, b: Quaternion) -> Vector3 {
        let a = self;
        let num12 = b.x() + b.x();
        let num2 = b.y() + b.y();
        let num = b.z() + b.z();
        let num11 = b.w() * num12;
        let num10 = b.w() * num2;
        let num9 = b.w() * num;
        let num8 = b.x() * num12;
        let num7 = b.x() * num2;
        let num6 = b.x() * num;
        let num5 = b.y() * num2;
        let num4 = b.y() * num;
        let num3 = b.z() * num;
        let num15 = ((a.x() * ((1.0 - num5) - num3)) + (a.y() * (num7 - num9))) + (a.z() * (num6 + num10));
        let num14 = ((a.x() * (num7 + num9)) + (a.y() * ((1.0 - num8) - num3))) + (a.z() * (num4 - num11));
        let num13 = ((a.x() * (num6 - num10)) + (a.y() * (num4 + num11))) + (a.z() * ((1.0 - num8) - num5));
        Vector3::new(num15, num14, num13)
    }
}

impl Div<f64> for Vector3 {
    type Output = Vector3;
    fn div(self, b: f64) -> Vector3 {
        Vector3::new(self.x() / b, self.y() / b, self.z() / b)
    }
}

/// `TTriangle`.
pub type Triangle = [u32; 3];
/// `TStrip`.
pub type Strip = Vec<u32>;

/// `TBoundSphere`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BoundSphere {
    pub center: Vector3,
    pub radius: f32,
}

impl BoundSphere {
    /// `SetNone`.
    pub fn set_none(&mut self) {
        self.center = Vector3::default();
        self.radius = 0.0;
    }
}

/// `TTransform`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Transform {
    pub translation: Vector3,
    pub rotation: Quaternion,
    pub scale: f32,
}

impl Transform {
    /// `SetNone`.
    pub fn set_none(&mut self) {
        self.translation = Vector3::default();
        self.rotation.set_identity();
        self.scale = 1.0;
    }

    /// `IsNone`.
    pub fn is_none(&self) -> bool {
        self.translation.is_zero() && self.rotation.is_identity() && same_value(f64::from(self.scale), 1.0)
    }

    pub fn none() -> Transform {
        let mut transform = Transform::default();
        transform.set_none();
        transform
    }
}

impl Mul<Transform> for Vector3 {
    type Output = Vector3;
    fn mul(self, b: Transform) -> Vector3 {
        self * b.rotation * f64::from(b.scale) + b.translation
    }
}

impl Mul<Transform> for BoundSphere {
    type Output = BoundSphere;
    fn mul(self, b: Transform) -> BoundSphere {
        BoundSphere {
            center: self.center * b,
            radius: (f64::from(self.radius) * f64::from(b.scale)) as f32,
        }
    }
}

impl Mul for Transform {
    type Output = Transform;
    fn mul(self, b: Transform) -> Transform {
        Transform {
            translation: self.translation + b.translation * self.rotation * f64::from(self.scale),
            rotation: self.rotation * b.rotation,
            scale: (f64::from(self.scale) * f64::from(b.scale)) as f32,
        }
    }
}

/// Delphi `ArcCos`: `ArcTan2(Sqrt((1 + X) * (1 - X)), X)`.
pub fn arc_cos(x: f64) -> f64 {
    ((1.0 + x) * (1.0 - x)).sqrt().atan2(x)
}

/// Delphi `ArcSin`: `ArcTan2(X, Sqrt((1 + X) * (1 - X)))`.
pub fn arc_sin(x: f64) -> f64 {
    x.atan2(((1.0 + x) * (1.0 - x)).sqrt())
}

// Delphi's `Sin` and `Cos` on Win64 (the RTL the 4.1.5q builds link): an
// argument reduction to [-π/4, π/4] and two kernels, read from the machine
// code of `Sniff.exe` (`System.Sin` and `System.Cos` call them). The results
// differ from the C runtime's and from the correctly rounded ones by an ulp
// at times (`Sin(Pi / 4)`, and the sine of half of 110.221202 degrees), which
// flips the sign or the last bit of a matrix element near zero after
// `AxisAngleToM33`. Every operation is one double operation, in the order of
// the machine code.

const fn bits(value: u64) -> f64 {
    f64::from_bits(value)
}

const QUARTER_PI: f64 = bits(0x3FE9_21FB_5444_2D18);
const THREE_QUARTERS_PI: f64 = bits(0x4002_D97C_7F33_21D2);
const FIVE_QUARTERS_PI: f64 = bits(0x400F_6A7A_2955_385E);
/// π/2 in three parts for the reduction of `|x| <= 5π/4`.
const HALF_PI: [f64; 3] = [
    bits(0x3FF9_21FB_5444_2D18),
    bits(0x3C91_A626_3314_5C04),
    bits(0x3967_0734_4A40_9382),
];
/// -π/2 in three parts for the reduction of `|x| < 2^22`.
const MINUS_HALF_PI: [f64; 3] = [
    bits(0xBFF9_21FB_5440_0000),
    bits(0xBDD0_B461_1A60_0000),
    bits(0xBBA3_198A_2E03_7073),
];
const TWO_OVER_PI: f64 = bits(0x3FE4_5F30_6DC9_C883);
const REDUCTION_LIMIT: f64 = bits(0x4150_0000_0000_0000);
const COS: [f64; 6] = [
    bits(0xBDA8_FA6A_8A7D_84DF),
    bits(0x3E21_EE9D_C12C_88AC),
    bits(0xBE92_7E4F_7F1E_E922),
    bits(0x3EFA_01A0_19C8_F945),
    bits(0xBF56_C16C_16C1_5018),
    bits(0x3FA5_5555_5555_554B),
];
const SIN: [f64; 8] = [
    bits(0x3DE5_E0A2_8E7F_A626),
    bits(0xBE5A_E600_81AA_5E86),
    bits(0x3EC7_1DE3_7936_614A),
    bits(0xBF2A_01A0_19E8_0E58),
    bits(0xBC29_D73D_6337_65DD),
    bits(0x3F81_1111_1111_0BA5),
    bits(0x3C6A_74A9_34AD_37D5),
    bits(0xBFC5_5555_5555_5555),
];

/// The cosine kernel: the cosine of `x + y`, `|x| <= π/4`.
fn kernel_cos(x: f64, y: f64) -> f64 {
    let z = x * x;
    let w = z * z;
    let a = ((COS[0] * w + COS[2]) * w + COS[4]) * z;
    let b = (COS[1] * w + COS[3]) * w + COS[5];
    let p = (b + a) * w;
    let hz = z * 0.5;
    let w1 = 1.0 - hz;
    let t = hz + (w1 - 1.0);
    w1 + ((p - x * y) - t)
}

/// The sine kernel: the sine of `x + y`, `|x| <= π/4`.
fn kernel_sin(x: f64, y: f64) -> f64 {
    let z = x * x;
    let w = z * z;
    let v = z * x;
    let a = ((SIN[0] * w + SIN[2]) * w + SIN[4]) + SIN[5];
    let b = (SIN[1] * w + SIN[3]) * w + SIN[6];
    let r = ((a * z + b) + SIN[7]) * v + (1.0 - z * 0.5) * y;
    x + r
}

/// The reduction: `x` less `n` times π/2 as `y0 + y1`, with `n & 3`.
/// `None` for `|x| >= 2^22`, which takes a path that is not ported (no
/// rotation gets there).
fn reduce(x: f64) -> Option<(i64, f64, f64)> {
    let ax = x.abs();
    if QUARTER_PI >= ax {
        return Some((0, x, 0.0));
    }
    if FIVE_QUARTERS_PI >= ax {
        let mut n: i64 = if THREE_QUARTERS_PI >= ax { 1 } else { 2 };
        if 0.0 > x {
            n = -n;
        }
        let f = n as f64;
        let (t0, t1, t2) = (f * HALF_PI[0], f * HALF_PI[1], f * HALF_PI[2]);
        let r = x - t0;
        let y0 = r - t1;
        let y1 = ((-t1) - (y0 - r)) - t2;
        return Some((n & 3, y0, y1));
    }
    if REDUCTION_LIMIT > ax {
        let mut n = (((ax - QUARTER_PI) * TWO_OVER_PI) as i64) + 1;
        if 0.0 > x {
            n = -n;
        }
        let f = n as f64;
        let (t0, t1, t2) = (f * MINUS_HALF_PI[0], f * MINUS_HALF_PI[1], f * MINUS_HALF_PI[2]);
        let r = x + t0;
        // Two double-double additions: t1 + r, then t2 + that.
        let s = t1 + r;
        let bb = s - t1;
        let error = (t1 - (s - bb)) + (r - bb);
        let hi = s + error;
        let lo = error - (hi - s);
        let s2 = t2 + hi;
        let bb2 = s2 - t2;
        let error2 = ((t2 - (s2 - bb2)) + (hi - bb2)) + lo;
        let hi2 = s2 + error2;
        let lo2 = error2 - (hi2 - s2);
        return Some((n & 3, hi2, lo2));
    }
    None
}

/// Delphi's `Sin` (`cosine` false) and `Cos`.
fn sin_cos(x: f64, cosine: bool) -> f64 {
    if x.is_nan() {
        return x;
    }
    if QUARTER_PI > x.abs() {
        return if cosine { kernel_cos(x, 0.0) } else { kernel_sin(x, 0.0) };
    }
    let Some((n, y0, y1)) = reduce(x) else {
        return if cosine { x.cos() } else { x.sin() };
    };
    match (n + i64::from(cosine)) & 3 {
        0 => kernel_sin(y0, y1),
        1 => kernel_cos(y0, y1),
        2 => -kernel_sin(y0, y1),
        _ => -kernel_cos(y0, y1),
    }
}

/// Delphi `Sin` on Win64.
pub fn sin(x: f64) -> f64 {
    sin_cos(x, false)
}

/// Delphi `Cos` on Win64.
pub fn cos(x: f64) -> f64 {
    sin_cos(x, true)
}

/// Delphi `RadToDeg`.
pub fn rad_to_deg(radians: f64) -> f64 {
    radians * (180.0 / std::f64::consts::PI)
}

/// Delphi `DegToRad`.
pub fn deg_to_rad(degrees: f64) -> f64 {
    degrees * (std::f64::consts::PI / 180.0)
}

/// `Normalize(var x, y, z)`.
fn normalize3(x: &mut f64, y: &mut f64, z: &mut f64) {
    let s = (*x * *x + *y * *y + *z * *z).sqrt();
    if s > 0.0 {
        *x /= s;
        *y /= s;
        *z /= s;
    }
}

/// `IdentityM33`.
pub fn identity_m33() -> Matrix33 {
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
}

/// `M33ToEuler`.
pub fn m33_to_euler(m: &Matrix33) -> (f64, f64, f64) {
    if m[0][2] < 1.0 {
        if m[0][2] > -1.0 {
            ((-m[1][2]).atan2(m[2][2]), arc_sin(m[0][2]), (-m[0][1]).atan2(m[0][0]))
        } else {
            (-(-m[1][0]).atan2(m[1][1]), -std::f64::consts::PI / 2.0, 0.0)
        }
    } else {
        (m[1][0].atan2(m[1][1]), std::f64::consts::PI / 2.0, 0.0)
    }
}

/// `EulerToM33`.
pub fn euler_to_m33(x: f64, y: f64, z: f64) -> Matrix33 {
    if same_value(x, 0.0) && same_value(y, 0.0) && same_value(z, 0.0) {
        return identity_m33();
    }
    let (sin_x, cos_x) = (sin(x), cos(x));
    let (sin_y, cos_y) = (sin(y), cos(y));
    let (sin_z, cos_z) = (sin(z), cos(z));
    [
        [cos_y * cos_z, -cos_y * sin_z, sin_y],
        [
            sin_x * sin_y * cos_z + sin_z * cos_x,
            cos_x * cos_z - sin_x * sin_y * sin_z,
            -sin_x * cos_y,
        ],
        [
            sin_x * sin_z - cos_x * sin_y * cos_z,
            cos_x * sin_y * sin_z + sin_x * cos_z,
            cos_x * cos_y,
        ],
    ]
}

/// `M33ToQuaternion`.
pub fn m33_to_quaternion(m: &Matrix33) -> Quaternion {
    const NEXT: [usize; 3] = [1, 2, 0];
    let mut quat = Quaternion::default();
    let trace = m[0][0] + m[1][1] + m[2][2];
    let q = &mut quat.q;
    if trace > 0.0 {
        let mut root = (trace + 1.0).sqrt();
        q[0] = root / 2.0;
        root = 0.5 / root;
        q[1] = (m[2][1] - m[1][2]) * root;
        q[2] = (m[0][2] - m[2][0]) * root;
        q[3] = (m[1][0] - m[0][1]) * root;
    } else {
        let mut i = if m[1][1] > m[0][0] { 1 } else { 0 };
        if m[2][2] > m[i][i] {
            i = 2;
        }
        let j = NEXT[i];
        let k = NEXT[j];
        let mut root = (m[i][i] - m[j][j] - m[k][k] + 1.0).sqrt();
        q[i + 1] = root / 2.0;
        root = 0.5 / root;
        q[0] = (m[k][j] - m[j][k]) * root;
        q[j + 1] = (m[j][i] + m[i][j]) * root;
        q[k + 1] = (m[k][i] + m[i][k]) * root;
    }
    quat
}

/// `QuaternionToM33`.
pub fn quaternion_to_m33(quat: &Quaternion) -> Matrix33 {
    let q = &quat.q;
    let f_tx = 2.0 * q[1];
    let f_ty = 2.0 * q[2];
    let f_tz = 2.0 * q[3];
    let f_twx = f_tx * q[0];
    let f_twy = f_ty * q[0];
    let f_twz = f_tz * q[0];
    let f_txx = f_tx * q[1];
    let f_txy = f_ty * q[1];
    let f_txz = f_tz * q[1];
    let f_tyy = f_ty * q[2];
    let f_tyz = f_tz * q[2];
    let f_tzz = f_tz * q[3];
    [
        [1.0 - (f_tyy + f_tzz), f_txy - f_twz, f_txz + f_twy],
        [f_txy + f_twz, 1.0 - (f_txx + f_tzz), f_tyz - f_twx],
        [f_txz - f_twy, f_tyz + f_twx, 1.0 - (f_txx + f_tyy)],
    ]
}

/// `QuaternionToAxisAngle`: the angle and the axis.
pub fn quaternion_to_axis_angle(quat: &Quaternion) -> (f64, f64, f64, f64) {
    let q = &quat.q;
    let squared_length = q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if squared_length > 0.0 {
        let a = arc_cos(q[0]) * 2.0;
        let length = squared_length.sqrt();
        (a, q[1] / length, q[2] / length, q[3] / length)
    } else {
        (0.0, 1.0, 0.0, 0.0)
    }
}

/// `AxisAngleToQuaternion`.
pub fn axis_angle_to_quaternion(a: f64, mut x: f64, mut y: f64, mut z: f64) -> Quaternion {
    normalize3(&mut x, &mut y, &mut z);
    let s = sin(a / 2.0);
    Quaternion::new(cos(a / 2.0), s * x, s * y, s * z)
}

/// `EulerToQuaternion`.
pub fn euler_to_quaternion(x: f64, y: f64, z: f64) -> Quaternion {
    m33_to_quaternion(&euler_to_m33(x, y, z))
}

/// `QuaternionToEuler`.
pub fn quaternion_to_euler(quat: &Quaternion) -> (f64, f64, f64) {
    m33_to_euler(&quaternion_to_m33(quat))
}

/// `M33ToAxisAngle`.
pub fn m33_to_axis_angle(m: &Matrix33) -> (f64, f64, f64, f64) {
    quaternion_to_axis_angle(&m33_to_quaternion(m))
}

/// `AxisAngleToM33`.
pub fn axis_angle_to_m33(a: f64, x: f64, y: f64, z: f64) -> Matrix33 {
    quaternion_to_m33(&axis_angle_to_quaternion(a, x, y, z))
}

/// `Vector3Cross`.
pub fn vector3_cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        (a.y() * b.z()) - (b.y() * a.z()),
        (a.z() * b.x()) - (b.z() * a.x()),
        (a.x() * b.y()) - (b.x() * a.y()),
    )
}

/// `Vector3Dot`.
pub fn vector3_dot(a: Vector3, b: Vector3) -> f64 {
    (a.x() * b.x()) + (a.y() * b.y()) + (a.z() * b.z())
}

/// `Tris2Indices`.
pub fn tris2_indices(tris: &[Triangle]) -> Vec<u32> {
    tris.iter().flatten().copied().collect()
}

/// `Indices2Tris`.
pub fn indices2_tris(indices: &[u32]) -> Vec<Triangle> {
    indices.as_chunks::<3>().0.to_vec()
}

/// `Indices2Strip`.
pub fn indices2_strip(indices: &[u32]) -> Strip {
    indices.to_vec()
}

/// `Indices2Strips`.
pub fn indices2_strips(indices: &[u32]) -> Vec<Strip> {
    vec![indices2_strip(indices)]
}

/// `TriangulateStrip`: the triangles of a strip without the degenerate ones.
pub fn triangulate_strip(strip: &[u32]) -> Vec<Triangle> {
    let mut result = Vec::new();
    if strip.len() < 3 {
        return result;
    }
    let mut b = strip[0];
    let mut c = strip[1];
    let mut flip = false;
    for &next in &strip[2..] {
        let a = b;
        b = c;
        c = next;
        if a != b && b != c && c != a {
            result.push(if flip { [a, c, b] } else { [a, b, c] });
        }
        flip = !flip;
    }
    result
}

/// `TriangulateStrips`.
pub fn triangulate_strips(strips: &[Strip]) -> Vec<Triangle> {
    strips.iter().flat_map(|strip| triangulate_strip(strip)).collect()
}

/// `CalculateMinMax`.
pub fn calculate_min_max(verts: &[Vector3]) -> (Vector3, Vector3) {
    let mut vmin = Vector3::new(MAX_SINGLE, MAX_SINGLE, MAX_SINGLE);
    let mut vmax = Vector3::new(-MAX_SINGLE, -MAX_SINGLE, -MAX_SINGLE);
    for v in verts {
        for axis in 0..3 {
            if v.v[axis] < vmin.v[axis] {
                vmin.v[axis] = v.v[axis];
            }
            if v.v[axis] > vmax.v[axis] {
                vmax.v[axis] = v.v[axis];
            }
        }
    }
    (vmin, vmax)
}

/// `CalculateCenterRadius`.
pub fn calculate_center_radius(verts: &[Vector3], from_min_max: bool, max_radius: bool) -> (Vector3, f64) {
    let mut center = Vector3::default();
    let mut r = 0.0;
    if verts.is_empty() {
        return (center, r);
    }
    if from_min_max {
        let (vmin, vmax) = calculate_min_max(verts);
        center = Vector3::new(
            (vmin.x() + vmax.x()) / 2.0,
            (vmin.y() + vmax.y()) / 2.0,
            (vmin.z() + vmax.z()) / 2.0,
        );
    } else {
        for &v in verts {
            center = center + v;
        }
        center = center / verts.len() as f64;
    }
    if !max_radius {
        r = MAX_SINGLE;
    }
    for &v in verts {
        let rv = (center - v).length();
        if (max_radius && rv > r) || (!max_radius && rv < r) {
            r = rv;
        }
    }
    (center, r)
}

fn check_triangle(tri: &Triangle, count: usize) -> R<()> {
    if tri.iter().any(|&index| index as usize >= count) {
        return Err(DfError::new(format!(
            "Triangle ({}, {}, {}) exceeds the number of vertices {count}",
            tri[0], tri[1], tri[2]
        )));
    }
    Ok(())
}

/// `CalculateFaceNormals`: the normal of each vertex as the normalized sum
/// of the faces it is in.
pub fn calculate_face_normals(verts: &[Vector3], triangles: &[Triangle]) -> R<Vec<Vector3>> {
    let mut norms = vec![Vector3::default(); verts.len()];
    for tri in triangles {
        check_triangle(tri, verts.len())?;
        let a = verts[tri[0] as usize];
        let b = verts[tri[1] as usize];
        let c = verts[tri[2] as usize];
        let face = vector3_cross(b - a, c - a);
        for &index in tri {
            norms[index as usize] = norms[index as usize] + face;
        }
    }
    for norm in &mut norms {
        norm.normalize();
    }
    Ok(norms)
}

/// `OrthogonalizeTangent`.
fn orthogonalize_tangent(tangent: &mut Vector3, binormal: &mut Vector3, normal: Vector3) {
    const K_NORMALIZE_EPSILON: f64 = 1e-6;
    let x_axis = Vector3::new(1.0, 0.0, 0.0);
    let y_axis = Vector3::new(0.0, 1.0, 0.0);
    let z_axis = Vector3::new(0.0, 0.0, 1.0);

    let n_dot_t = vector3_dot(normal, *tangent);
    let mut new_tangent = Vector3::new(
        tangent.x() - n_dot_t * normal.x(),
        tangent.y() - n_dot_t * normal.y(),
        tangent.z() - n_dot_t * normal.z(),
    );
    let mag_t = new_tangent.length();
    new_tangent.normalize_by(mag_t);

    let n_dot_b = vector3_dot(normal, *binormal);
    let t_dot_b = vector3_dot(new_tangent, *binormal) * mag_t;
    let mut new_binormal = Vector3::new(
        binormal.x() - n_dot_b * normal.x() - t_dot_b * new_tangent.x(),
        binormal.y() - n_dot_b * normal.y() - t_dot_b * new_tangent.y(),
        binormal.z() - n_dot_b * normal.z() - t_dot_b * new_tangent.z(),
    );
    let mag_b = new_binormal.length();
    new_binormal.normalize_by(mag_b);

    if mag_t <= K_NORMALIZE_EPSILON || mag_b <= K_NORMALIZE_EPSILON {
        // Create the tangent basis from scratch.
        let dp_xn = vector3_dot(x_axis, normal).abs();
        let dp_yn = vector3_dot(y_axis, normal).abs();
        let dp_zn = vector3_dot(z_axis, normal).abs();
        let (axis1, axis2) = if dp_xn <= dp_yn && dp_xn <= dp_zn {
            (x_axis, if dp_yn <= dp_zn { y_axis } else { z_axis })
        } else if dp_yn <= dp_xn && dp_yn <= dp_zn {
            (y_axis, if dp_xn <= dp_zn { x_axis } else { z_axis })
        } else {
            (z_axis, if dp_xn <= dp_yn { x_axis } else { y_axis })
        };
        let new_tangent = axis1 - normal * vector3_dot(normal, axis1);
        *tangent = new_tangent;
        tangent.normalize();
        let new_binormal = axis2 - normal * vector3_dot(normal, axis2) - *tangent * vector3_dot(new_tangent, axis2);
        *binormal = new_binormal;
        binormal.normalize();
    } else {
        *tangent = new_tangent;
        *binormal = new_binormal;
    }
}

/// `CalculateTangentsBitangents`, the NifSkope version. The texture
/// coordinates that are NaN become zero, as upstream changes them in place.
pub fn calculate_tangents_bitangents(
    verts: &[Vector3],
    norms: &[Vector3],
    texco: &mut [Vector2],
    triangles: &[Triangle],
) -> R<(Vec<Vector3>, Vec<Vector3>)> {
    let mut tan = vec![Vector3::default(); verts.len()];
    let mut bin = vec![Vector3::default(); verts.len()];
    for tri in triangles {
        check_triangle(tri, verts.len())?;
        let (i1, i2, i3) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        let (v1, v2, v3) = (verts[i1], verts[i2], verts[i3]);
        for index in [i1, i2, i3] {
            texco[index].validate_nan();
        }
        let (w1, w2, w3) = (texco[i1], texco[i2], texco[i3]);
        let v2v1 = v2 - v1;
        let v3v1 = v3 - v1;
        let w2w1 = w2 - w1;
        let w3w1 = w3 - w1;
        let mut r = w2w1.v[0] * w3w1.v[1] - w3w1.v[0] * w2w1.v[1];
        r = if r >= 0.0 { 1.0 } else { -1.0 };
        let mut sdir = Vector3::default();
        let mut tdir = Vector3::default();
        for axis in 0..3 {
            sdir.v[axis] = (w3w1.v[1] * v2v1.v[axis] - w2w1.v[1] * v3v1.v[axis]) * r;
            tdir.v[axis] = (w2w1.v[0] * v3v1.v[axis] - w3w1.v[0] * v2v1.v[axis]) * r;
        }
        sdir.normalize();
        tdir.normalize();
        for &index in tri {
            tan[index as usize] = tan[index as usize] + tdir;
            bin[index as usize] = bin[index as usize] + sdir;
        }
    }
    for i in 0..verts.len() {
        let n = norms[i];
        let t = &mut tan[i];
        let b = &mut bin[i];
        if t.is_zero() || b.is_zero() {
            *t = Vector3::new(n.v[1], n.v[2], n.v[0]);
            *b = vector3_cross(n, *t);
        } else {
            t.normalize();
            *t = *t - n * vector3_dot(n, *t);
            t.normalize();
            b.normalize();
            *b = *b - n * vector3_dot(n, *b);
            *b = *b - *t * vector3_dot(*t, *b);
            b.normalize();
        }
    }
    Ok((tan, bin))
}

/// `CalculateTangentsBitangents2`: NifSkope's equation with Unity's
/// weighting by area and angle and its orthonormalization. The normals and
/// texture coordinates that are NaN become zero, as upstream changes them
/// in place.
pub fn calculate_tangents_bitangents2(
    verts: &[Vector3],
    norms: &mut [Vector3],
    texco: &mut [Vector2],
    triangles: &[Triangle],
) -> R<(Vec<Vector3>, Vec<Vector3>)> {
    const K_NEXT_INDEX: [[usize; 2]; 3] = [[2, 1], [0, 2], [1, 0]];
    let mut tan = vec![Vector3::default(); verts.len()];
    let mut bin = vec![Vector3::default(); verts.len()];
    for tri in triangles {
        check_triangle(tri, verts.len())?;
        let tri_vertex = [verts[tri[0] as usize], verts[tri[1] as usize], verts[tri[2] as usize]];
        for &index in tri {
            texco[index as usize].validate_nan();
        }
        let (w1, w2, w3) = (texco[tri[0] as usize], texco[tri[1] as usize], texco[tri[2] as usize]);
        let v2v1 = tri_vertex[1] - tri_vertex[0];
        let v3v1 = tri_vertex[2] - tri_vertex[0];
        let w2w1 = w2 - w1;
        let w3w1 = w3 - w1;
        let mut r = w2w1.v[0] * w3w1.v[1] - w3w1.v[0] * w2w1.v[1];
        let area_mult = if r < 0.0 { -r } else { r };
        let mut tangent = Vector3::default();
        let mut binormal = Vector3::default();
        if area_mult >= 1e-8 {
            r = 1.0 / r;
            for axis in 0..3 {
                tangent.v[axis] = (w2w1.v[0] * v3v1.v[axis] - w3w1.v[0] * v2v1.v[axis]) * r;
                binormal.v[axis] = (w3w1.v[1] * v2v1.v[axis] - w2w1.v[1] * v3v1.v[axis]) * r;
            }
            // Weight by area.
            tangent.normalize();
            tangent = tangent * area_mult;
            binormal.normalize();
            binormal = binormal * area_mult;
        }
        for v in 0..3 {
            let mut edge1 = tri_vertex[K_NEXT_INDEX[v][0]] - tri_vertex[v];
            let mut edge2 = tri_vertex[K_NEXT_INDEX[v][1]] - tri_vertex[v];
            // Weight by angle.
            edge1.normalize();
            edge2.normalize();
            let angle = (edge1.x() * edge2.x() + edge1.y() * edge2.y() + edge1.z() * edge2.z()).clamp(-1.0, 1.0);
            let w = arc_cos(angle);
            let index = tri[v] as usize;
            for axis in 0..3 {
                tan[index].v[axis] += w * tangent.v[axis];
                bin[index].v[axis] += w * binormal.v[axis];
            }
        }
    }
    for i in 0..verts.len() {
        orthogonalize_tangent(&mut tan[i], &mut bin[i], norms[i]);
        tan[i].validate_nan();
        bin[i].validate_nan();
        norms[i].validate_nan();
    }
    Ok((tan, bin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_and_cosine_of_delphi() {
        // `Sin` and `Cos` of half the angle as the xEdit GUI's script
        // adapter returns them (the bits of 20097 such pairs checked): the C
        // runtime is an ulp above at 90 degrees, the correctly rounded value
        // an ulp below at 110.221202 and 646.876437.
        for (degrees, sine, cosine) in [
            (90.0, 0x3FE6_A09E_667F_3BCC, 0x3FE6_A09E_667F_3BCD),
            (110.221202, 0x3FEA_3F8D_1F13_945A, 0x3FE2_4DC6_9B5F_31D5),
            (95.890933, 0x3FE7_C29F_55E5_AC41, 0x3FE5_6F4F_4E1E_D5E0),
            (180.0, 0x3FF0_0000_0000_0000, 0x3C91_A626_3314_5C07),
            (-437.008794, 0x3FE3_EC21_C3A8_A8DC, 0xBFE9_0ABC_11AF_349C),
            (-484.950803, 0x3FEC_60C4_A1B6_D122, 0xBFDD_9387_224F_9572),
            (48.201302, 0x3FDA_2241_95D5_09EE, 0x3FED_35E6_4A65_CB5D),
            (646.876437, 0xBFE3_0FE2_DD4A_3D6A, 0x3FE9_B3EF_F270_F6A1),
        ] {
            let half = deg_to_rad(degrees) / 2.0;
            assert_eq!(sin(half).to_bits(), sine, "sin of {degrees} / 2");
            assert_eq!(cos(half).to_bits(), cosine, "cos of {degrees} / 2");
        }
    }

    #[test]
    fn identity_rotation_round_trips() {
        let q = axis_angle_to_quaternion(0.0, 1.0, 0.0, 0.0);
        let (a, x, y, z) = quaternion_to_axis_angle(&q);
        assert_eq!((a, x, y, z), (0.0, 1.0, 0.0, 0.0));
        let m = quaternion_to_m33(&q);
        assert_eq!(m, identity_m33());
    }

    #[test]
    fn strips_drop_degenerate_triangles() {
        assert_eq!(triangulate_strip(&[0, 1, 2, 2, 3, 4]), vec![[0, 1, 2], [2, 4, 3]]);
        assert_eq!(triangulate_strip(&[0, 1, 2, 3]), vec![[0, 1, 2], [1, 3, 2]]);
    }
}
