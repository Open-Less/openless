package com.openless.app

/**
 * Checks a Rime user dictionary snapshot (`pinyin_simp.userdb.txt`, the text
 * format librime's backup writes) before it is merged. Rime merges a
 * snapshot into whichever dictionary its header names, so anything that is
 * not this keyboard's pinyin dictionary is refused here.
 */
internal object RimeUserDictionaryFile {
    const val MAX_BYTES = 32 * 1024 * 1024

    /**
     * Entries in [text] that still count as learned (commit count above zero;
     * Rime keeps deleted words as negative counts). Throws
     * IllegalArgumentException when [text] is not a pinyin user dictionary.
     */
    fun entryCount(text: String): Int {
        val lines = text.lineSequence().map { it.trimEnd('\r') }
        var dbName: String? = null
        var dbType: String? = null
        var entries = 0
        for (line in lines) {
            when {
                line.startsWith("#@/db_name\t") -> dbName = line.substringAfter('\t')
                line.startsWith("#@/db_type\t") -> dbType = line.substringAfter('\t')
                line.isBlank() || line.startsWith("#") -> Unit
                else -> {
                    val columns = line.split('\t')
                    require(columns.size >= 2 && columns[0].isNotBlank() && columns[1].isNotEmpty()) { "Not a user dictionary row" }
                    if (commits(columns.getOrNull(2)) > 0) entries++
                }
            }
        }
        require(dbName == "pinyin_simp" && dbType == "userdb") { "Not a pinyin user dictionary" }
        return entries
    }

    private fun commits(value: String?): Int =
        value?.split(' ')?.firstOrNull { it.startsWith("c=") }?.substring(2)?.toIntOrNull() ?: 1
}
