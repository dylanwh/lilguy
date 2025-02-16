
#[macro_export]
macro_rules! io_methods {
    ($methods:ident, $field:ident) => {
        use tokio::io::AsyncBufReadExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        $methods.add_async_method_mut("write", |_, mut this, args: LuaMultiValue| async move {
            let mut buf = Vec::new();
            for arg in args {
                match arg {
                    LuaValue::String(s) => buf.extend_from_slice(&s.as_bytes()),
                    LuaValue::Integer(i) => buf.extend_from_slice(i.to_string().as_bytes()),
                    LuaValue::Number(n) => buf.extend_from_slice(n.to_string().as_bytes()),
                    _ => return Err(LuaError::external("invalid argument")),
                }
            }
            let rv = this.$field.get_mut().write_all(&buf).await?;
            Ok(rv)
        });

        $methods.add_async_method_mut("read_exact", |_, mut this, len: usize| async move {
            let mut buf = Vec::with_capacity(len);
            this.$field
                .read_exact(&mut buf)
                .await
                .map_err(LuaError::external)?;
            Ok(buf)
        });

        $methods.add_async_method_mut("read_line", |lua, mut this, _: ()| async move {
            let mut buf = Vec::new();
            this.$field.read_until(b'\n', &mut buf).await?;
            lua.create_string(&buf)
        });

        $methods.add_async_method_mut("read_until", |lua, mut this, byte: u8| async move {
            let mut buf = Vec::new();
            this.$field.read_until(byte, &mut buf).await?;
            lua.create_string(&buf)
        });

        $methods.add_async_method_mut("read_to_end", |lua, mut this, _: ()| async move {
            let mut buf = Vec::new();
            this.$field.read_to_end(&mut buf).await?;
            lua.create_string(&buf)
        });

        $methods.add_async_method_mut("flush", |_, mut this, _: ()| async move {
            this.$field.get_mut().flush().await?;
            Ok(())
        });

        $methods.add_async_method_mut("close", |_, mut this, _: ()| async move {
            this.$field.get_mut().shutdown().await.map_err(LuaError::external)?;
            Ok(())
        });
    };
}
