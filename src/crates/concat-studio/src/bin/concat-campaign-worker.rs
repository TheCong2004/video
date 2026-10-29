// SPDX-License-Identifier: AGPL-3.0-or-later
//! Explicit background campaign process. Does not require an open GUI window.
use concat_studio::campaign::{
    engine::{Engine, EngineSettings},
    CampaignStore,
};
use std::path::PathBuf;
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!(
            "concat-campaign-worker {} protocol=1",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if args.len()==5 && args[0]=="--verify-index-video" {
        let settings:EngineSettings=serde_json::from_slice(&std::fs::read(&args[1]).map_err(|_|"Không đọc được cấu hình")?).map_err(|_|"Cấu hình không hợp lệ")?;
        let path=PathBuf::from(&args[2]);
        let seconds=args[3].parse::<f64>().map_err(|_|"Thời lượng không hợp lệ")?;
        if !seconds.is_finite()||!(1.0..=600.0).contains(&seconds)||!path.is_absolute(){return Err("Đường dẫn hoặc thời lượng không hợp lệ".into());}
        let source=url::Url::parse(&args[4]).map_err(|_|"Nguồn video không hợp lệ")?;
        if source.scheme()!="https"||source.host_str()!=Some("grok.com")||!source.path().starts_with("/imagine/post/"){return Err("Chỉ ghi nguồn post Grok đã chọn".into());}
        let asset=concat_studio::campaign::Asset{sha256:concat_studio::campaign::quality::hash(&path)?,path,rights:"operator supplied existing video".into(),provider:"existing-grok-local-edit".into()};
        let report=concat_studio::campaign::quality::verify(&asset,&settings.ffmpeg,seconds,&std::sync::atomic::AtomicBool::new(false))?;
        concat_studio::video_library::record_online(&settings.data_root.join("studio/video-library"),&asset.path,"Kedi Factory · Reused Grok",Some(source.as_str()))?;
        println!("{}",serde_json::to_string(&report).map_err(|_|"Không mã hóa được kết quả kiểm tra")?);
        println!("VIDEO_INDEXED={}",asset.path.display());return Ok(());
    }
    if args.len()==9 && args[0]=="--import-local-render" {
        use concat_studio::campaign::{Asset,CampaignState,JobState,Stage};
        let settings:EngineSettings=serde_json::from_slice(&std::fs::read(&args[1]).map_err(|_|"Không đọc được cấu hình")?).map_err(|_|"Cấu hình không hợp lệ")?;
        let store=CampaignStore::open(&PathBuf::from(&args[2]))?;let record=store.read(&args[3])?;
        let video=&args[4];let expected=args[5].parse::<u64>().map_err(|_|"Phiên bản không hợp lệ")?;
        let input=PathBuf::from(&args[6]);let output=PathBuf::from(&args[7]);
        if !input.is_absolute()||!output.is_absolute(){return Err("Cần đường dẫn file tuyệt đối".into());}
        let source=url::Url::parse(&args[8]).map_err(|_|"Nguồn video không hợp lệ")?;
        if source.scheme()!="https"||source.host_str()!=Some("grok.com")||!source.path().starts_with("/imagine/post/"){return Err("Nguồn phải là post Grok đã chọn".into());}
        let job=record.jobs.iter().find(|j|&j.id==video).ok_or("Video không tồn tại")?;
        let narration=job.script.as_ref().ok_or("Thiếu kịch bản")?.narration.trim();
        let imported_script=std::fs::read_to_string(output.parent().ok_or("Thiếu thư mục render")?.join("script.txt")).map_err(|_|"Thiếu kịch bản đi kèm video")?;
        if imported_script.trim()!=narration{return Err("Video không đi kèm đúng kịch bản của mục đã chọn".into());}
        let media=Asset{sha256:concat_studio::campaign::quality::hash(&input)?,path:input,rights:"operator supplied existing video".into(),provider:"existing-grok".into()};
        let rendered=Asset{sha256:concat_studio::campaign::quality::hash(&output)?,path:output,rights:"operator authorized local edit".into(),provider:"local-edit".into()};
        let report=concat_studio::campaign::quality::verify(&rendered,&settings.ffmpeg,record.plan().video_seconds,&std::sync::atomic::AtomicBool::new(false))?;
        store.transact(&args[3],|r|{
            if r.state!=CampaignState::Paused||r.plan().publish{return Err("Chỉ nhập kết quả local vào chiến dịch đã dừng, không đăng bài".into());}
            let job=r.jobs.iter_mut().find(|j|&j.id==video).ok_or("Video không tồn tại")?;
            if job.revision!=expected||job.lease.is_some()||job.publish_dispatched||!matches!(job.stage,Stage::Media|Stage::Render|Stage::Quality){return Err("Video đã đổi hoặc còn tác vụ đang chạy".into());}
            job.media=vec![media.clone()];job.rendered=Some(rendered.clone());job.quality=Some(report.clone());job.stage=Stage::Done;job.state=JobState::Completed;job.revision+=1;job.error=None;job.retry_at=0;job.attempt=0;job.hold=false;job.approval_revision=None;
            r.audit("operator","existing_local_render_imported",Some(video),format!("Nhập video đã dựng và đạt QC từ {source}; không gọi Grok tạo mới, không xác nhận các dispatch cloud chưa rõ kết quả"),concat_studio::campaign::store::now());Ok(())
        })?;
        concat_studio::video_library::record_online(&settings.data_root.join("studio/video-library"),&rendered.path,"Kedi Factory · Reused Grok",Some(source.as_str()))?;
        println!("LOCAL_VIDEO_COMPLETED={video}");println!("{}",serde_json::to_string(&report).map_err(|_|"Không mã hóa được kiểm tra")?);return Ok(());
    }
    if args.len()==6 && args[0]=="--recover-content" {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply { #[serde(rename="requestId")] request_id:String, result:concat_studio::campaign::Script }
        let metadata=std::fs::metadata(&args[5]).map_err(|_|"Không đọc được tệp phản hồi")?;
        if !metadata.is_file() || metadata.len()>65536{return Err("Tệp phản hồi vượt giới hạn".into());}
        let reply:Reply=serde_json::from_slice(&std::fs::read(&args[5]).map_err(|_|"Không đọc được phản hồi")?).map_err(|_|"Phản hồi JSON không đúng cấu trúc kịch bản")?;
        let revision=args[4].parse::<u64>().map_err(|_|"Phiên bản video không hợp lệ")?;
        let store=CampaignStore::open(&PathBuf::from(&args[1]))?;
        store.recover_content(&args[2],&args[3],revision,&reply.request_id,reply.result)?;
        println!("CONTENT_RECOVERED={}",args[3]);
        return Ok(());
    }
    if args.len()==6 && args[0]=="--discard-content" {
        let store=CampaignStore::open(&PathBuf::from(&args[1]))?;
        store.discard_content_reply(&args[2],&args[3],args[4].parse::<u64>().map_err(|_|"Phiên bản không hợp lệ")?,&args[5])?;
        println!("OLD_CONTENT_REPLY_DISCARDED={}",args[3]);return Ok(());
    }
    if args.len()==5 && args[0]=="--retry-undispatched-media" {
        use concat_studio::campaign::{CampaignState,JobState,Stage};
        let store=CampaignStore::open(&PathBuf::from(&args[1]))?;
        let r=store.read(&args[2])?;let video=&args[3];
        let expected=args[4].parse::<u64>().map_err(|_|"Phiên bản không hợp lệ")?;
        let job=r.jobs.iter().find(|j|&j.id==video).ok_or("Video không tồn tại")?;
        if r.state!=CampaignState::Paused||job.revision!=expected||job.lease.is_some()||job.stage!=Stage::Media||job.state!=JobState::Unknown||job.error.as_deref()!=Some("LEASE_CORRELATION_INVALID")||!job.media.is_empty(){return Err("Không xác minh được lỗi trước dispatch".into());}
        let root=store.directory(&args[2])?.join(video);
        let entries=std::fs::read_dir(root.join("concat-production")).map_err(|_|"Thiếu biên nhận")?.collect::<Result<Vec<_>,_>>().map_err(|_|"Không đọc được biên nhận")?;
        if entries.len()!=1{return Err("Có nhiều biên nhận; cần đối chiếu trước khi chạy lại".into());}
        let file=entries[0].path().join("manifest.json");
        if std::fs::metadata(&file).map_err(|_|"Thiếu biên nhận")?.len()>1048576{return Err("Biên nhận quá lớn".into());}
        let manifest:serde_json::Value=serde_json::from_slice(&std::fs::read(file).map_err(|_|"Không đọc được biên nhận")?).map_err(|_|"Biên nhận không hợp lệ")?;
        if manifest["state"]!="FAILED"||manifest["error_code"]!="LEASE_CORRELATION_INVALID"||!manifest["pending_submission"].is_null()||manifest["stages"].as_array().is_none_or(|s|!s.is_empty())||manifest["plan"]["page"]["browser_profile_id"]!=r.plan().providers.grok_profile||manifest["plan"]["page"]["output_root"].as_str()!=root.to_str(){return Err("Chưa chứng minh được không có dispatch; không gửi lại".into());}
        store.manual_edit(&args[2],video,expected,None,None)?;store.hold(&args[2],video,false)?;
        println!("VERIFIED_PREDISPATCH_MEDIA_RETRY={video}");return Ok(());
    }
    if args.len()==4 && args[0]=="--enable-local-auto" {
        let store=CampaignStore::open(&PathBuf::from(&args[1]))?;
        let record=store.read(&args[2])?;
        if record.state!=concat_studio::campaign::CampaignState::Paused||record.plan().publish{return Err("Chỉ áp dụng tự động cho chiến dịch video local đã tạm dừng và không đăng bài".into());}
        let expected=args[3].parse::<u64>().map_err(|_|"Phiên bản không hợp lệ")?;
        let mut plan=record.plan().clone();plan.mode=concat_studio::campaign::Automation::FullAuto;plan.approve_content=false;
        let revised=store.revise(&args[2],plan,expected,true)?;
        store.approve(&args[2],revised.revision,&revised.plan().hash())?;
        println!("LOCAL_AUTO_APPROVED_PUBLISH_DISABLED");return Ok(());
    }
    if args.len()==5 && args[0]=="--bind-media-profile" {
        let store=CampaignStore::open(&PathBuf::from(&args[1]))?;
        let revision=args[3].parse::<u64>().map_err(|_|"Phiên bản chiến dịch không hợp lệ")?;
        store.bind_grok_source(&args[2],revision,&args[4])?;
        println!("MEDIA_SOURCE_UPDATED");return Ok(());
    }
    if args.len()==5 && args[0]=="--approve-plan" {
        let store=CampaignStore::open(&PathBuf::from(&args[1]))?;
        store.approve(&args[2],args[3].parse::<u64>().map_err(|_|"Phiên bản không hợp lệ")?,&args[4])?;
        println!("PLAN_APPROVED");return Ok(());
    }
    if args.len()==3 && args[0]=="--open-grok" {
        let settings:EngineSettings=serde_json::from_slice(&std::fs::read(&args[1]).map_err(|_|"Không đọc được cấu hình")?).map_err(|_|"Cấu hình không hợp lệ")?;
        let browser=concat_studio::nexora::NexoraSettings::load(&settings.config_root).and_then(|s|s.client()).map_err(|_|"Kết nối Nexora không hợp lệ")?;
        let profile=browser.profiles().map_err(|_|"Không đọc được hồ sơ Nexora")?.into_iter().find(|p|p.id==args[2]).ok_or("Hồ sơ không tồn tại")?;
        browser.ensure_production_grok(&profile).map_err(|e|format!("Chưa xác minh được cửa sổ Grok: {e:?}; không tự mở lại"))?;
        println!("GROK_PAGE_OPENED");
        println!("GROK_READY={}",browser.production_browser_ready(&profile.id).map_err(|_|"Không đọc được trạng thái worker Grok")?);
        return Ok(());
    }
    if args.len()==2 && matches!(args[0].as_str(),"--inspect-media-source"|"--start-media-source") {
        let settings:EngineSettings=serde_json::from_slice(&std::fs::read(&args[1]).map_err(|_|"Không đọc được cấu hình worker")?).map_err(|_|"Cấu hình worker không hợp lệ")?;
        let nexora=concat_studio::nexora::NexoraSettings::load(&settings.config_root).map_err(|_|"Kết nối Nexora không hợp lệ")?;
        let profiles=if args[0]=="--start-media-source" {
            nexora.start().map_err(|e|format!("Không khởi động được Nexora: {e:?}"))?
        }else{
            let browser=nexora.client().map_err(|_|"Kết nối Nexora không hợp lệ")?;
            browser.health().map_err(|e|format!("Nexora chưa sẵn sàng: {e:?}"))?;
            browser.profiles().map_err(|_|"Không đọc được hồ sơ Nexora")?
        };
        let browser=nexora.client().map_err(|_|"Kết nối Nexora không hợp lệ")?;
        println!("{}",serde_json::to_string(&profiles.iter().map(|p|serde_json::json!({"id":p.id,"name":p.name,"grok_ready":browser.production_browser_ready(&p.id).ok()})).collect::<Vec<_>>()).map_err(|_|"Không mã hóa được hồ sơ")?);
        return Ok(());
    }
    if args.len()==4 && args[0]=="--control" && matches!(args[3].as_str(),"pause"|"cancel") {
        let store=CampaignStore::open(&PathBuf::from(&args[1]))?;
        let record=store.control(&args[2],&args[3])?;
        println!("CAMPAIGN_STATE={:?}",record.state);
        return Ok(());
    }
    if args.len() != 4 || !matches!(args[0].as_str(), "--run" | "--start-run" | "--preflight") {
        return Err(
            "usage: concat-campaign-worker --run|--preflight STORE_ROOT CAMPAIGN_ID SETTINGS_JSON"
                .into(),
        );
    }
    let settings = std::fs::read(PathBuf::from(&args[3])).map_err(|e| e.to_string())?;
    if settings.len() > 32768 {
        return Err("Engine settings exceed limit".into());
    }
    let settings: EngineSettings = serde_json::from_slice(&settings).map_err(|e| e.to_string())?;
    let store = CampaignStore::open(&PathBuf::from(&args[1]))?;
    let engine = Engine::new(settings);
    if args[0] == "--preflight" {
        engine.preflight(store.read(&args[2])?.plan())?;
        println!("PREFLIGHT_OK");
        Ok(())
    } else {
        if args[0]=="--start-run" {
            engine.preflight(store.read(&args[2])?.plan())?;
            store.control(&args[2],"start")?;
        }
        concat_studio::campaign::worker::run(store.clone(), &args[2], engine).inspect_err(|error| {
            let _ = store.control(&args[2], "pause");
            let _ = store.transact(&args[2], |r| {
                r.audit(
                    "worker",
                    "preflight_blocked",
                    None,
                    error.clone(),
                    concat_studio::campaign::store::now(),
                );
                Ok(())
            });
        })
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
