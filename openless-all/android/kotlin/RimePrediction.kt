package com.openless.app

/** Rank only candidates matching the current encoding, without changing the default commit. */
internal object RimePrediction {
    private const val CONTEXT_LENGTH = 8

    fun append(context: String, committed: String): String {
        val text = context + committed
        val count = text.codePointCount(0, text.length)
        return text.substring(text.offsetByCodePoints(0, (count - CONTEXT_LENGTH).coerceAtLeast(0)))
    }

    fun rank(rows: List<String>, context: String): List<String> {
        if (rows.size < 3 || context.isEmpty()) return rows
        val predictions = runCatching {
            val values = OpenLessRime.request("predict", "", context).getJSONArray("candidates")
            (0 until values.length()).associate { values.getString(it) to it }
        }.getOrDefault(emptyMap())
        return promote(rows, predictions)
    }

    fun promote(rows: List<String>, predictions: Map<String, Int>): List<String> {
        if (predictions.isEmpty()) return rows
        return rows.take(1) + rows.drop(1).sortedBy { predictions[it] ?: Int.MAX_VALUE }
    }
}
