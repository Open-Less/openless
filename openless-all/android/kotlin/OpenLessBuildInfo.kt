package com.openless.app

/**
 * Manually bumped by 0.01 before each debug build during this iterative
 * on-device testing cycle — not the app's real release version — so the
 * keyboard settings screen can confirm at a glance which build is actually
 * installed. Also doubles as the "did a new build get installed" signal
 * OpenLessApplication uses to reset the "unclean shutdown" stat, since a
 * count from a previous build isn't meaningful to keep comparing against.
 */
object OpenLessBuildInfo {
    const val VERSION = "1.79"
}
