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
  private const val ALIAS = "mutsuna_echo_elevenlabs_api_key"
  private const val PREFS = "secure_credentials"
  private const val VALUE = "elevenlabs_api_key"
  private const val TRANSFORMATION = "AES/GCM/NoPadding"

  @JvmStatic fun save(context: Context, secret: String) {
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(Cipher.ENCRYPT_MODE, getOrCreateKey())
    val encrypted = cipher.doFinal(secret.toByteArray(StandardCharsets.UTF_8))
    val payload = Base64.encodeToString(cipher.iv, Base64.NO_WRAP) + "." +
      Base64.encodeToString(encrypted, Base64.NO_WRAP)
    check(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(VALUE, payload).commit()) {
      "暗号化したAPIキーを端末へ保存できませんでした。"
    }
  }

  @JvmStatic fun has(context: Context): Boolean =
    context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).contains(VALUE)

  @JvmStatic fun load(context: Context): String? {
    val payload = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(VALUE, null) ?: return null
    val parts = payload.split('.', limit = 2)
    if (parts.size != 2) return null
    val cipher = Cipher.getInstance(TRANSFORMATION)
    cipher.init(Cipher.DECRYPT_MODE, getKey(), GCMParameterSpec(128, Base64.decode(parts[0], Base64.NO_WRAP)))
    val plain = cipher.doFinal(Base64.decode(parts[1], Base64.NO_WRAP))
    return try { String(plain, StandardCharsets.UTF_8) } finally { plain.fill(0) }
  }

  @JvmStatic fun delete(context: Context) {
    check(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().remove(VALUE).commit()) {
      "保存済みAPIキーを削除できませんでした。"
    }
    val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    if (store.containsAlias(ALIAS)) store.deleteEntry(ALIAS)
  }

  private fun getKey(): SecretKey {
    val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    return store.getKey(ALIAS, null) as? SecretKey ?: throw IllegalStateException("暗号鍵がありません。")
  }

  private fun getOrCreateKey(): SecretKey = try { getKey() } catch (_: Exception) {
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    generator.init(KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
      .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
      .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
      .setKeySize(256)
      .build())
    generator.generateKey()
  }
}
