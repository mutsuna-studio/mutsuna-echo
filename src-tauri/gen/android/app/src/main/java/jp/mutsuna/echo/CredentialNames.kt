package jp.mutsuna.echo

internal object CredentialNames {
  private val supported = setOf(
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

  fun alias(credential: String): String = "mutsuna_echo_${validate(credential)}_api_key"

  fun value(credential: String): String = "${validate(credential)}_api_key"

  private fun validate(credential: String): String {
    require(credential in supported) { "対応していない認証情報です。" }
    return credential
  }
}
