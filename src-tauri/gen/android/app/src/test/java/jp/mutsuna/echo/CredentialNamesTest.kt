package jp.mutsuna.echo

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class CredentialNamesTest {
  @Test
  fun allNativeCredentialIdsHaveStableNames() {
    val credentials = listOf(
      "elevenlabs",
      "soniox",
      "cloudflare-oauth-access-token",
      "cloudflare-oauth-refresh-token",
      "cloudflare-oauth-expires-at",
      "cloudflare-oauth-account-id",
      "cloudflare-oauth-account-name",
      "cloudflare-oauth-accounts",
      "mutsuna-cloud-access-token",
    )

    credentials.forEach { credential ->
      assertEquals("mutsuna_echo_${credential}_api_key", CredentialNames.alias(credential))
      assertEquals("${credential}_api_key", CredentialNames.value(credential))
    }
  }

  @Test
  fun unknownCredentialIdIsRejected() {
    assertThrows(IllegalArgumentException::class.java) {
      CredentialNames.alias("unknown-provider")
    }
  }
}
