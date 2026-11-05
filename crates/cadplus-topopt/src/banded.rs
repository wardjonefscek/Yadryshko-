//! Ленточная матрица и разложение Холецкого.
//!
//! Узлы сетки нумеруются по столбцам, поэтому ширина ленты — около `2·(nely + 2)`,
//! а разложение стоит `n · b²` операций. Для нервюр (десятки тысяч неизвестных,
//! лента ~100) это доли секунды, без внешних библиотек и без итераций.

/// Симметричная положительно определённая ленточная матрица (хранится нижняя лента).
#[derive(Clone, Debug)]
pub struct Banded {
    n: usize,
    b: usize,
    /// Строка `i` хранит элементы `(i, j)` для `j` от `i − b` до `i`; индекс `j − (i − b)`.
    data: Vec<f64>,
}

impl Banded {
    /// Нулевая матрица размера `n` с полушириной ленты `b`.
    pub fn zeros(n: usize, b: usize) -> Self {
        Self {
            n,
            b,
            data: vec![0.0; n * (b + 1)],
        }
    }

    /// Размер.
    pub fn n(&self) -> usize {
        self.n
    }

    /// Обнулить, сохранив размер (для повторной сборки на каждой итерации).
    pub fn clear(&mut self) {
        self.data.fill(0.0);
    }

    /// Прибавить `v` к элементу `(i, j)`, где `j ≤ i` и `i − j ≤ b`.
    #[inline]
    pub fn add_lower(&mut self, i: usize, j: usize, v: f64) {
        debug_assert!(j <= i && i - j <= self.b);
        let k = j + self.b - i;
        self.data[i * (self.b + 1) + k] += v;
    }

    /// Разложение Холецкого на месте: A = L·Lᵀ. Ошибка — матрица не положительно определена
    /// (например, конструкция не закреплена).
    pub fn cholesky(&mut self) -> Result<(), usize> {
        let (n, b) = (self.n, self.b);
        let w = b + 1;
        for i in 0..n {
            let j0 = i.saturating_sub(b);
            let aii = self.data[i * w + b];
            for j in j0..=i {
                // Общая часть строк i и j: k от max(i−b, j−b) до j−1.
                let k0 = j0.max(j.saturating_sub(b));
                let mut sum = self.data[i * w + (j + b - i)];
                if k0 < j {
                    let ri = &self.data[i * w + (k0 + b - i)..i * w + (j + b - i)];
                    let rj = &self.data[j * w + (k0 + b - j)..j * w + b];
                    sum -= dot(ri, rj);
                }
                if j == i {
                    // Ведущий элемент, исчезающе малый относительно исходной диагонали, —
                    // признак свободного движения тела как целого (нет закреплений).
                    if !sum.is_finite() || sum <= 1e-10 * aii.abs() || sum <= 0.0 {
                        return Err(i);
                    }
                    self.data[i * w + b] = sum.sqrt();
                } else {
                    self.data[i * w + (j + b - i)] = sum / self.data[j * w + b];
                }
            }
        }
        Ok(())
    }

    /// Решить L·Lᵀ·x = rhs после `cholesky`.
    pub fn solve_in_place(&self, x: &mut [f64]) {
        let (n, b) = (self.n, self.b);
        let w = b + 1;
        for i in 0..n {
            let j0 = i.saturating_sub(b);
            let row = &self.data[i * w + (j0 + b - i)..i * w + b];
            let s = x[i] - dot(row, &x[j0..i]);
            x[i] = s / self.data[i * w + b];
        }
        for i in (0..n).rev() {
            x[i] /= self.data[i * w + b];
            let xi = x[i];
            let j0 = i.saturating_sub(b);
            for (j, l) in (j0..i).zip(&self.data[i * w + (j0 + b - i)..i * w + b]) {
                x[j] -= l * xi;
            }
        }
    }
}

#[inline]
fn dot(a: &[f64], b: &[f64]) -> f64 {
    // Четыре накопителя — компилятор векторизует, порядок сложения фиксирован.
    let mut s = [0.0_f64; 4];
    let chunks = a.len() / 4;
    for c in 0..chunks {
        for (l, acc) in s.iter_mut().enumerate() {
            *acc += a[4 * c + l] * b[4 * c + l];
        }
    }
    let mut t = (s[0] + s[1]) + (s[2] + s[3]);
    for k in 4 * chunks..a.len() {
        t += a[k] * b[k];
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_tridiagonal() {
        // Матрица 1D-стержня: 2 на диагонали, −1 рядом. Решение для rhs = (1, 0, …, 0, 1) — все единицы.
        let n = 50;
        let mut a = Banded::zeros(n, 1);
        for i in 0..n {
            a.add_lower(i, i, 2.0);
            if i > 0 {
                a.add_lower(i, i - 1, -1.0);
            }
        }
        a.cholesky().unwrap();
        let mut x = vec![0.0; n];
        x[0] = 1.0;
        x[n - 1] = 1.0;
        a.solve_in_place(&mut x);
        for v in x {
            assert!((v - 1.0).abs() < 1e-12);
        }
    }

    #[test]
    fn detects_singular() {
        let mut a = Banded::zeros(2, 1);
        a.add_lower(0, 0, 1.0);
        a.add_lower(1, 0, 1.0);
        a.add_lower(1, 1, 1.0);
        assert_eq!(a.cholesky(), Err(1));
    }
}
