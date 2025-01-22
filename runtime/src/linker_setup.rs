/// Setup common linker functions
fn setup_linker(
    context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
    linker: &mut Linker<State>,
) -> Result<()> {
    let context_len = context.clone();
    let context_input = context.clone();
    
    linker
        .func_wrap(
            "env",
            "__host_len",
            move |mut _caller: Caller<'_, State>| -> i32 {
                match context_len.lock() {
                    Ok(ctx) => ctx.block.len() as i32 + 4,
                    Err(_) => i32::MAX, // Signal error
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap __host_len: {:?}", e))?;

    linker
        .func_wrap(
            "env",
            "__load_input",
            move |mut caller: Caller<'_, State>, data_start: i32| {
                let mem = match caller.get_export("memory") {
                    Some(export) => match export.into_memory() {
                        Some(memory) => memory,
                        None => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    },
                    None => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                };

                let (input, height) = match context_input.lock() {
                    Ok(ctx) => (ctx.block.clone(), ctx.height),
                    Err(_) => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                };

                let input_clone = match try_into_vec(height.to_le_bytes()) {
                    Ok(mut v) => {
                        v.extend(input);
                        v
                    }
                    Err(_) => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                };

                let sz = to_usize_or_trap(&mut caller, data_start);
                if sz == usize::MAX {
                    caller.data_mut().had_failure = true;
                    return;
                }

                if let Err(_) = mem.write(&mut caller, sz, input_clone.as_slice()) {
                    caller.data_mut().had_failure = true;
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap __load_input: {:?}", e))?;

    linker
        .func_wrap(
            "env",
            "__log",
            |mut caller: Caller<'_, State>, data_start: i32| {
                let mem = match caller.get_export("memory") {
                    Some(export) => match export.into_memory() {
                        Some(memory) => memory,
                        None => return,
                    },
                    None => return,
                };

                let data = mem.data(&caller);
                let bytes = match try_read_arraybuffer_as_vec(data, data_start) {
                    Ok(v) => v,
                    Err(_) => return,
                };

                if let Ok(text) = std::str::from_utf8(&bytes) {
                    print!("{}", text);
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap __log: {:?}", e))?;

    linker
        .func_wrap(
            "env",
            "abort",
            |mut caller: Caller<'_, State>, _: i32, _: i32, _: i32, _: i32| {
                caller.data_mut().had_failure = true;
            },
        )
        .map_err(|e| anyhow!("Failed to wrap abort: {:?}", e))?;

    Ok(())
}

/// Setup view-specific linker functions
fn setup_linker_view(
    context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
    linker: &mut Linker<State>,
) -> Result<()> {
    let context_get = context.clone();
    let context_get_len = context.clone();
    
    linker
        .func_wrap(
            "env",
            "__flush",
            move |_caller: Caller<'_, State>, _encoded: i32| {},
        )
        .map_err(|e| anyhow!("Failed to wrap __flush: {:?}", e))?;

    linker
        .func_wrap(
            "env",
            "__get",
            move |mut caller: Caller<'_, State>, key: i32, value: i32| {
                let mem = match caller.get_export("memory") {
                    Some(export) => match export.into_memory() {
                        Some(memory) => memory,
                        None => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    },
                    None => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                };

                let data = mem.data(&caller);
                let height = match context_get.clone().lock() {
                    Ok(ctx) => ctx.height,
                    Err(_) => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                };

                match try_read_arraybuffer_as_vec(data, key) {
                    Ok(key_vec) => {
                        match Self::db_value_at_block(context_get.clone(), &key_vec, height) {
                            Ok(lookup) => {
                                if let Err(_) = mem.write(&mut caller, value as usize, lookup.as_slice()) {
                                    caller.data_mut().had_failure = true;
                                }
                            }
                            Err(_) => {
                                caller.data_mut().had_failure = true;
                            }
                        }
                    }
                    Err(_) => {
                        if let Ok(error_bits) = try_into_vec(i32::MAX.to_le_bytes()) {
                            if let Err(_) = mem.write(
                                &mut caller,
                                (value - 4) as usize,
                                error_bits.as_slice(),
                            ) {
                                caller.data_mut().had_failure = true;
                            }
                        } else {
                            caller.data_mut().had_failure = true;
                        }
                    }
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap __get: {:?}", e))?;

    linker
        .func_wrap(
            "env",
            "__get_len",
            move |mut caller: Caller<'_, State>, key: i32| -> i32 {
                let mem = match caller.get_export("memory") {
                    Some(export) => match export.into_memory() {
                        Some(memory) => memory,
                        None => return i32::MAX,
                    },
                    None => return i32::MAX,
                };

                let data = mem.data(&caller);
                let height = match context_get_len.clone().lock() {
                    Ok(ctx) => ctx.height,
                    Err(_) => return i32::MAX,
                };

                match try_read_arraybuffer_as_vec(data, key) {
                    Ok(key_vec) => {
                        match Self::db_value_at_block(context_get_len.clone(), &key_vec, height) {
                            Ok(value) => value.len() as i32,
                            Err(_) => i32::MAX,
                        }
                    }
                    Err(_) => i32::MAX,
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap __get_len: {:?}", e))?;

    Ok(())
}

/// Setup indexer-specific linker functions
fn setup_linker_indexer(
    context: Arc<Mutex<MetashrewRuntimeContext<T>>>,
    linker: &mut Linker<State>,
) -> Result<()> {
    let context_ref = context.clone();
    let context_get = context.clone();
    let context_get_len = context.clone();
    
    linker
        .func_wrap(
            "env",
            "__flush",
            move |mut caller: Caller<'_, State>, encoded: i32| {
                let height = match context_ref.clone().lock() {
                    Ok(ctx) => ctx.height,
                    Err(_) => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                };

                let mem = match caller.get_export("memory") {
                    Some(export) => match export.into_memory() {
                        Some(memory) => memory,
                        None => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    },
                    None => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                };

                let data = mem.data(&caller);
                let encoded_vec = match try_read_arraybuffer_as_vec(data, encoded) {
                    Ok(v) => v,
                    Err(_) => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                };

                let mut batch = T::Batch::default();
                if let Err(_) = Self::db_create_empty_update_list(&mut batch, height) {
                    caller.data_mut().had_failure = true;
                    return;
                }

                let decoded = match KeyValueFlush::parse_from_bytes(&encoded_vec) {
                    Ok(d) => d,
                    Err(_) => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                };

                for (k, v) in decoded.list.iter().tuples() {
                    let k_owned = k.clone();
                    let v_owned = v.clone();

                    if let Err(_) = Self::db_append_annotated(
                        context_ref.clone(),
                        &mut batch,
                        &k_owned,
                        &v_owned,
                        height,
                    ) {
                        caller.data_mut().had_failure = true;
                        return;
                    }

                    match try_into_vec(height.to_le_bytes()) {
                        Ok(update_key) => {
                            if let Err(_) = Self::db_append(context_ref.clone(), &mut batch, &update_key, &k_owned) {
                                caller.data_mut().had_failure = true;
                                return;
                            }
                        }
                        Err(_) => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    }
                }

                debug!(
                    "saving {:?} k/v pairs for block {:?}",
                    decoded.list.len() / 2,
                    height
                );

                match context_ref.clone().lock() {
                    Ok(mut ctx) => {
                        ctx.state = 1;
                        if let Err(_) = ctx.db.write(batch) {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    }
                    Err(_) => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap __flush: {:?}", e))?;

    linker
        .func_wrap(
            "env",
            "__get",
            move |mut caller: Caller<'_, State>, key: i32, value: i32| {
                let mem = match caller.get_export("memory") {
                    Some(export) => match export.into_memory() {
                        Some(memory) => memory,
                        None => {
                            caller.data_mut().had_failure = true;
                            return;
                        }
                    },
                    None => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                };

                let data = mem.data(&caller);
                let height = match context_get.clone().lock() {
                    Ok(ctx) => ctx.height,
                    Err(_) => {
                        caller.data_mut().had_failure = true;
                        return;
                    }
                };

                match try_read_arraybuffer_as_vec(data, key) {
                    Ok(key_vec) => {
                        match Self::db_value_at_block(context_get.clone(), &key_vec, height) {
                            Ok(lookup) => {
                                if let Err(_) = mem.write(&mut caller, value as usize, lookup.as_slice()) {
                                    caller.data_mut().had_failure = true;
                                }
                            }
                            Err(_) => {
                                caller.data_mut().had_failure = true;
                            }
                        }
                    }
                    Err(_) => {
                        if let Ok(error_bits) = try_into_vec(i32::MAX.to_le_bytes()) {
                            if let Err(_) = mem.write(
                                &mut caller,
                                (value - 4) as usize,
                                error_bits.as_slice(),
                            ) {
                                caller.data_mut().had_failure = true;
                            }
                        } else {
                            caller.data_mut().had_failure = true;
                        }
                    }
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap __get: {:?}", e))?;

    linker
        .func_wrap(
            "env",
            "__get_len",
            move |mut caller: Caller<'_, State>, key: i32| -> i32 {
                let mem = match caller.get_export("memory") {
                    Some(export) => match export.into_memory() {
                        Some(memory) => memory,
                        None => return i32::MAX,
                    },
                    None => return i32::MAX,
                };

                let data = mem.data(&caller);
                let height = match context_get_len.clone().lock() {
                    Ok(ctx) => ctx.height,
                    Err(_) => return i32::MAX,
                };

                match try_read_arraybuffer_as_vec(data, key) {
                    Ok(key_vec) => {
                        match Self::db_value_at_block(context_get_len.clone(), &key_vec, height) {
                            Ok(value) => value.len() as i32,
                            Err(_) => i32::MAX,
                        }
                    }
                    Err(_) => i32::MAX,
                }
            },
        )
        .map_err(|e| anyhow!("Failed to wrap __get_len: {:?}", e))?;

    Ok(())
}