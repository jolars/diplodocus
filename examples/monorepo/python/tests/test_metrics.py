import math
import unittest

import tinystats


class MetricsTest(unittest.TestCase):
    def test_model_comparison(self):
        actual = [1.0, 2.0, 3.0, 4.0]
        predicted = [1.0, 2.0, 3.0, 6.0]
        self.assertEqual(tinystats.residuals(actual, predicted), [0.0, 0.0, 0.0, -2.0])
        self.assertEqual(tinystats.mean_absolute_error(actual, predicted), 0.5)
        self.assertEqual(tinystats.mean_squared_error(actual, predicted), 1.0)
        self.assertEqual(tinystats.root_mean_squared_error(actual, predicted), 1.0)
        self.assertEqual(tinystats.mean_error(actual, predicted), -0.5)
        self.assertAlmostEqual(tinystats.r_squared(actual, predicted), 0.2)

    def test_perfect_predictions_and_constant_baseline(self):
        actual = [1.0, 2.0, 3.0]
        self.assertEqual(tinystats.r_squared(actual, actual), 1.0)
        self.assertEqual(tinystats.r_squared(actual, [2.0] * 3), 0.0)
        for name in tinystats.__all__:
            if name not in ("residuals", "r_squared"):
                self.assertEqual(getattr(tinystats, name)(actual, actual), 0.0)

    def test_invalid_pairs(self):
        for name in tinystats.__all__:
            for actual, predicted in [([], []), ([1.0], [1.0, 2.0])]:
                with self.subTest(metric=name, actual=actual):
                    with self.assertRaises(ValueError):
                        getattr(tinystats, name)(actual, predicted)
        with self.assertRaises(ValueError):
            tinystats.r_squared([1.0, 1.0], [1.0, 2.0])

    def test_missing_values_are_not_silently_dropped(self):
        self.assertTrue(math.isnan(tinystats.mean_squared_error([1.0, math.nan], [1.0, 2.0])))


if __name__ == "__main__":
    unittest.main()
