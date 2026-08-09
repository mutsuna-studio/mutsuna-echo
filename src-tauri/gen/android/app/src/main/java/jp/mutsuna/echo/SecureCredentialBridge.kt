package jp.mutsuna.echo

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.nio.charset.StandardCharsets
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

object SecureCredentialBridge {
  private const val PREFS = "secure_credentials"
  private const val TRANSFORMATION = "AES/GCM/NoPadding"

  @JvmStatic fun save(context: Context, credential: String, secret: String) {
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(Cipher.ENCRYPT_MODE, getOrCreateKey(alias(credential)))
    val encrypted = cipher.doFinal(secret.toByteArray(StandardCharsets.UTF_8))
    val payload = Base64.encodeToString(cipher.iv, Base64.NO_WRAP) + "." +
      Base64.encodeToString(encrypted, Base64.NO_WRAP)
    check(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(value(credential), payload).commit()) {
      "暗号化したAPIキーを端末へ保存できませんでした。"
    }
  }

  @JvmStatic fun has(context: Context, credential: String): Boolean =
    context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).contains(value(credential))

  @JvmStatic fun load(context: Context, credential: String): String? {
    val payload = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(value(credential), null) ?: return null
    val parts = payload.split('.', limit = 2)
    if (parts.size != 2) return null
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(Cipher.DECRYPT_MODE, getKey(alias(credential)), GCMParameterSpec(128, Base64.decode(parts[0], Base64.NO_WRAP)))
    val plain = cipher.doFinal(Base64.decode(parts[1], Base64.NO_WRAP))
    return try { String(plain, StandardCharsets.UTF_8) } finally { plain.fill(0) }
  }

  @JvmStatic fun delete(context: Context, credential: String) {
    check(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().remove(value(credential)).commit()) {
      "保存済みAPIキーを削除できませんでした。"
    }
    val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    val alias = alias(credential)
    if (store.containsAlias(alias)) store.deleteEntry(alias)
  }

  private fun validate(credential: String): String = when (credential) {
    "elevenlabs", "soniox" -> credential
    else -> throw IllegalArgumentException("対応していない認証情報です。")
  }

  private fun alias(credential: String): String = "mutsuna_echo_${validate(credential)}_api_key"
  private fun value(credential: String): String = "${validate(credential)}_api_key"

  private fun getKey(alias: String): SecretKey {
    val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    return store.getKey(alias, null) as? SecretKey ?: throw IllegalStateException("暗号鍵がありません。")
  }

  private fun getOrCreateKey(alias: String): SecretKey = try { getKey(alias) } catch (_: Exception) {
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    generator.init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
      .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
      .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
      .setKeySize(256)
      .build())
    generator.generateKey()
  }
}
