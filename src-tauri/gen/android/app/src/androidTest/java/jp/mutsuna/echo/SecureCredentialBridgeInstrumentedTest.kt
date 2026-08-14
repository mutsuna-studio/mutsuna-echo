package jp.mutsuna.echo

import androidx.test.core.app.ApplicationProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class SecureCredentialBridgeInstrumentedTest {
  private val context = ApplicationProvider.getApplicationContext<android.content.Context>()
  private val credentials = listOf(
    "elevenlabs",
    "soniox",
    "cloudflare-oauth-access-token",
    "cloudflare-oauth-refresh-token",
    "cloudflare-oauth-expires-at",
    "cloudflare-oauth-account-id",
    "cloudflare-oauth-account-name",
    "cloudflare-oauth-accounts",
  )

  @After
  fun cleanUp() {
    credentials.forEach { credential ->
      if (SecureCredentialBridge.has(context, credential)) {
        SecureCredentialBridge.delete(context, credential)
      }
    }
  }

  @Test
  fun everyCredentialRoundTripsThroughAndroidKeystore() {
    credentials.forEachIndexed { index, credential ->
      val secret = "synthetic-${index}-日本語-!@#"

      assertFalse(SecureCredentialBridge.has(context, credential))
      assertNull(SecureCredentialBridge.load(context, credential))

      SecureCredentialBridge.save(context, credential, secret)

      assertTrue(SecureCredentialBridge.has(context, credential))
      assertEquals(secret, SecureCredentialBridge.load(context, credential))

      SecureCredentialBridge.delete(context, credential)
      assertFalse(SecureCredentialBridge.has(context, credential))
      assertNull(SecureCredentialBridge.load(context, credential))
    }
  }

  @Test
  fun overwritingCredentialDoesNotReturnPreviousSecret() {
    SecureCredentialBridge.save(context, "soniox", "synthetic-old")
    SecureCredentialBridge.save(context, "soniox", "synthetic-new")

    assertEquals("synthetic-new", SecureCredentialBridge.load(context, "soniox"))
  }
}
