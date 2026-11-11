import unittest

from query_meter import MeterInputError, QueryMeter


class PointBatch:
    def __init__(self, rows):
        self.rows = rows
        self.shape = (len(rows), 3)


class QueryMeterTests(unittest.TestCase):
    def test_counts_all_direct_calls_and_preserves_return_value(self):
        meter = QueryMeter()
        observed = []

        def field(points, scale=1.0):
            observed.append(points)
            return [scale * point[0] for point in points.rows]

        wrapped = meter.wrap("distance", field)
        first = PointBatch([(1, 2, 3), (4, 5, 6)])
        second = PointBatch([(7, 8, 9)])
        self.assertEqual(wrapped(first, scale=2.0), [2.0, 8.0])
        self.assertEqual(wrapped(second), [7.0])
        self.assertEqual(len(observed), 2)

        snapshot = meter.snapshot()["distance"]
        self.assertEqual(snapshot["calls_attempted"], 2)
        self.assertEqual(snapshot["points_submitted"], 3)
        self.assertEqual(snapshot["failed_calls"], 0)
        self.assertGreaterEqual(snapshot["wall_time_ns"], 0)

    def test_failed_evaluator_call_is_still_counted(self):
        meter = QueryMeter()

        def failing(points):
            raise RuntimeError("synthetic failure")

        wrapped = meter.wrap("gradient", failing)
        with self.assertRaisesRegex(RuntimeError, "synthetic failure"):
            wrapped(PointBatch([(0, 0, 0), (1, 1, 1)]))

        snapshot = meter.snapshot()["gradient"]
        self.assertEqual(snapshot["calls_attempted"], 1)
        self.assertEqual(snapshot["points_submitted"], 2)
        self.assertEqual(snapshot["failed_calls"], 1)

    def test_invalid_batch_shape_fails_before_invocation(self):
        meter = QueryMeter()
        wrapped = meter.wrap("distance", lambda points: points)
        with self.assertRaises(MeterInputError):
            wrapped([[1.0, 2.0], [3.0, 4.0]])
        self.assertEqual(meter.snapshot()["distance"]["calls_attempted"], 0)

    def test_named_evaluators_are_separate_and_totals_are_additive(self):
        meter = QueryMeter()
        distance = meter.wrap("distance", lambda points: [0.0] * len(points.rows))
        gradient = meter.wrap("gradient", lambda points: [(0.0, 0.0, 1.0)] * len(points.rows))
        batch = PointBatch([(0, 0, 0), (1, 0, 0)])
        distance(batch)
        gradient(batch)
        self.assertEqual(meter.snapshot()["distance"]["points_submitted"], 2)
        self.assertEqual(meter.snapshot()["gradient"]["points_submitted"], 2)
        self.assertEqual(meter.totals()["points_submitted"], 4)

    def test_duplicate_meter_names_are_rejected(self):
        meter = QueryMeter()
        meter.wrap("distance", lambda points: points)
        with self.assertRaisesRegex(ValueError, "already registered"):
            meter.wrap("distance", lambda points: points)


if __name__ == "__main__":
    unittest.main()
