package net.theavalanche.app

import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * Circular avatar for an [Account]. If the account has [Account.avatarData],
 * it is decoded and shown as a cropped circle. Otherwise a branded initial
 * placeholder is shown — matching iOS Sources/Views/Common/AccountAvatar.swift.
 *
 * @param account The account whose avatar to display.
 * @param size    Diameter of the circle in dp (matches the Swift `CGFloat size` param).
 */
@Composable
fun AccountAvatar(
    account: Account,
    size: Dp,
) {
    val avatarBitmap = remember(account.avatarData) {
        account.avatarData?.let { bytes ->
            runCatching {
                BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
            }.getOrNull()
        }
    }

    if (avatarBitmap != null) {
        Image(
            bitmap = avatarBitmap,
            contentDescription = account.displayName,
            contentScale = ContentScale.Crop,
            modifier = Modifier
                .size(size)
                .clip(CircleShape),
        )
    } else {
        val initial = account.displayName.take(1).uppercase()
        Box(
            contentAlignment = Alignment.Center,
            modifier = Modifier
                .size(size)
                .clip(CircleShape)
                .background(AvalancheColors.Brand.copy(alpha = 0.2f)),
        ) {
            Text(
                text = initial,
                color = AvalancheColors.Brand,
                // iOS uses size * 0.4 as the point size; approximate with sp from dp.
                fontSize = (size.value * 0.4f).sp,
                fontWeight = androidx.compose.ui.text.font.FontWeight.Medium,
            )
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun AccountAvatarInitialPreview() {
    AvalancheTheme {
        AccountAvatar(
            account = Account(id = "did:example:1", displayName = "Alice"),
            size = 48.dp,
        )
    }
}
