package com.openless.app

import android.content.Context

object OpenLessAppContext {
    @Volatile
    var context: Context? = null
        private set

    fun initialize(context: Context) {
        this.context = context.applicationContext
    }
}
