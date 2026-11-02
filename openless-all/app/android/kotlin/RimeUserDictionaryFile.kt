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
        var entries = 0
        parse(text) { _, commits -> if (commits != null && commits > 0) entries++ }
        return entries
    }

    /** The same snapshot without deletion markers or zero-count rows. Metadata (including /tick) is retained. */
    fun forImport(text: String): String = buildString {
        parse(text) { line, commits ->
            if (commits == null || commits > 0) append(line).append('\n')
        }
    }

    private fun parse(text: String, accept: (String, Int?) -> Unit) {
        // Match Rime's getline + trim_right: split only on LF, and trim ASCII whitespace.
        val lines = text.splitToSequence('\n').map { it.trimEnd(' ', '\t', '\r', '\u000b', '\u000c') }
        var dbName: String? = null
        var dbType: String? = null
        for (line in lines) {
            // Rime stops recognizing metadata after this directive; ignoring it here
            // would let a later, harmless-looking header hide a different target DB.
            require(line != "# no comment") { "User dictionary comment directives are not supported" }
            var commits: Int? = null
            when {
                line.startsWith("#@") -> {
                    val columns = line.substring(2).split('\t')
                    require(columns.size == 2) { "Invalid user dictionary metadata" }
                    when (columns[0]) {
                        "/db_name" -> {
                            require(dbName == null) { "Duplicate user dictionary name" }
                            dbName = columns[1]
                        }
                        "/db_type" -> {
                            require(dbType == null) { "Duplicate user dictionary type" }
                            dbType = columns[1]
                        }
                    }
                }
                line.isBlank() || line.startsWith("#") -> Unit
                else -> {
                    val columns = line.split('\t')
                    require(columns.size == 3 && columns[0].isNotBlank() && columns[1].isNotEmpty()) { "Not a user dictionary row" }
                    val counts = columns[2].split(' ').filter { it.startsWith("c=") }
                    require(counts.size == 1) { "Invalid user dictionary commit count" }
                    commits = counts.single().substring(2).toIntOrNull()
                    require(commits != null) { "Invalid user dictionary commit count" }
                }
            }
            accept(line, commits)
        }
        require(dbName == "pinyin_simp" && dbType == "userdb") { "Not a pinyin user dictionary" }
    }
}
