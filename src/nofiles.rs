use std::io;

pub fn set_nofiles(limit: u64) -> io::Result<()> {
    let (soft, hard) = rlimit::Resource::NOFILE.get()?;

    if soft > limit {
        info!("nNot increasing NOFILES ulimit: current soft ({}) is already higher than the specified ({})", soft, limit);
        return Ok(());
    }

    let mut setto = limit;

    if limit > hard {
        info!(
            "Requested NOFILES ({}) larger than the hard limit ({}), capping at {}.",
            limit, hard, hard
        );
        setto = hard;
    }

    if setto == soft {
        info!(
            "Requested NOFILES ({}) is the same as the current soft limit.",
            setto
        );
        return Ok(());
    }

    info!("Setting open files to {} (hard: {})", setto, hard);

    rlimit::Resource::NOFILE.set(setto, hard)?;

    Ok(())
}
