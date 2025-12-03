use ayjx::prelude::*;
use serde_json::{Value, json};

// ============================================================================
// NapCat 平台特有 API 实现
// ============================================================================

#[async_trait]
pub trait NapCatApi: Sync + Send {
    async fn call_api(
        &self,
        action: &str,
        params: Value,
        self_id: Option<&str>,
    ) -> AyjxResult<Value>;

    // ----- 账号相关 -----
    /// 设置账号信息
    /// nickname: 昵称
    /// personal_note: 个性签名
    /// sex: 性别 (0=未知, 1=男, 2=女)
    async fn set_qq_profile(
        &self,
        nickname: &str,
        personal_note: Option<&str>,
        sex: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "nickname": nickname,
        });

        if let Some(note) = personal_note {
            params["personal_note"] = json!(note);
        }

        if let Some(sex_str) = sex {
            params["sex"] = json!(sex_str);
        }

        self.call_api("set_qq_profile", params, self_id).await?;
        Ok(())
    }

    /// 获取被过滤的好友请求
    /// count: 获取数量，默认50
    async fn get_doubt_friends_add_request(
        &self,
        count: Option<u32>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({});
        if let Some(c) = count {
            params["count"] = json!(c);
        }
        self.call_api("get_doubt_friends_add_request", params, self_id)
            .await
    }

    /// 获取推荐好友/群聊卡片
    /// group_id: 群号（与 user_id 二选一）
    /// user_id: 用户QQ号（与 group_id 二选一）
    /// phone_number: 对方手机号（可选）
    async fn ark_share_peer_with_phone(
        &self,
        group_id: Option<&str>,
        user_id: Option<&str>,
        phone_number: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({});
        if let Some(gid) = group_id {
            params["group_id"] = json!(gid.parse::<i64>().unwrap_or(0));
        }
        if let Some(uid) = user_id {
            params["user_id"] = json!(uid.parse::<i64>().unwrap_or(0));
        }
        if let Some(phone) = phone_number {
            params["phoneNumber"] = json!(phone);
        }
        self.call_api("ArkSharePeer", params, self_id).await
    }

    /// 处理被过滤的好友请求
    /// flag: 请求标识符（从 get_doubt_friends_add_request 获取）
    /// approve: 是否同意（在 4.7.43 版本中该值无效，调用即表示同意）
    async fn set_doubt_friends_add_request(
        &self,
        flag: &str,
        approve: bool,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_doubt_friends_add_request",
            json!({
                "flag": flag,
                "approve": approve
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取当前账号在线客户端列表
    async fn get_online_clients(&self, self_id: Option<&str>) -> AyjxResult<Vec<String>> {
        let resp = self
            .call_api("get_online_clients", json!({}), self_id)
            .await?;
        if let Some(array) = resp.as_array() {
            Ok(array
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
        } else {
            Ok(Vec::new())
        }
    }

    /// 设置消息已读
    /// group_id: 群号（与 user_id 二选一）
    /// user_id: 用户QQ号（与 group_id 二选一）
    async fn mark_msg_as_read(
        &self,
        group_id: Option<&str>,
        user_id: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({});

        if let Some(gid) = group_id {
            params["group_id"] = json!(gid.parse::<i64>().unwrap_or(0));
        }

        if let Some(uid) = user_id {
            params["user_id"] = json!(uid.parse::<i64>().unwrap_or(0));
        }

        self.call_api("mark_msg_as_read", params, self_id).await?;
        Ok(())
    }

    /// 获取推荐群聊卡片
    /// group_id: 群号
    async fn ark_share_group(&self, group_id: &str, self_id: Option<&str>) -> AyjxResult<String> {
        let resp = self
            .call_api(
                "ArkShareGroup",
                json!({
                    "group_id": group_id.parse::<i64>().unwrap_or(0)
                }),
                self_id,
            )
            .await?;
        // 根据 OpenAPI 规范，返回的 data 字段是卡片 JSON 字符串
        if let Some(card_json) = resp.as_str() {
            Ok(card_json.to_string())
        } else {
            Ok(resp.to_string())
        }
    }

    /// 设置在线状态
    /// status: 10(在线), 30(离开), 50(忙碌), 60(Q我吧), 70(请勿打扰), 40(隐身)
    /// ext_status: 扩展状态，用于显示特定状态文本（如听歌中、春日限定等）
    /// battery_status: 电量状态
    async fn set_online_status(
        &self,
        status: i32,
        ext_status: i32,
        battery_status: i32,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_online_status",
            json!({
                "status": status,
                "ext_status": ext_status,
                "battery_status": battery_status
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 设置在线状态为"在线"
    async fn set_online(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 0, 0, self_id).await
    }

    /// 设置在线状态为"Q我吧"
    async fn set_q_me(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(60, 0, 0, self_id).await
    }

    /// 设置在线状态为"离开"
    async fn set_away(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(30, 0, 0, self_id).await
    }

    /// 设置在线状态为"忙碌"
    async fn set_busy(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(50, 0, 0, self_id).await
    }

    /// 设置在线状态为"请勿打扰"
    async fn set_dnd(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(70, 0, 0, self_id).await
    }

    /// 设置在线状态为"隐身"
    async fn set_invisible(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(40, 0, 0, self_id).await
    }

    /// 设置在线状态为"听歌中"
    async fn set_listening_music(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1028, 0, self_id).await
    }

    /// 设置在线状态为"春日限定"
    async fn set_spring_limited(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2037, 0, self_id).await
    }

    /// 设置在线状态为"一起元梦"
    async fn set_yuanmeng(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2025, 0, self_id).await
    }

    /// 设置在线状态为"求星搭子"
    async fn set_star_partner(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2026, 0, self_id).await
    }

    /// 设置在线状态为"被掏空"
    async fn set_exhausted(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2014, 0, self_id).await
    }

    /// 设置在线状态为"今日天气"
    async fn set_today_weather(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1030, 0, self_id).await
    }

    /// 设置在线状态为"我crash了"
    async fn set_crashed(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2019, 0, self_id).await
    }

    /// 设置在线状态为"爱你"
    async fn set_love_you(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2006, 0, self_id).await
    }

    /// 设置在线状态为"恋爱中"
    async fn set_in_love(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1051, 0, self_id).await
    }

    /// 设置在线状态为"好运锦鲤"
    async fn set_good_luck(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1071, 0, self_id).await
    }

    /// 设置在线状态为"水逆退散"
    async fn set_mercury_retrograde(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1201, 0, self_id).await
    }

    /// 设置在线状态为"嗨到飞起"
    async fn set_high_flying(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1056, 0, self_id).await
    }

    /// 设置在线状态为"元气满满"
    async fn set_full_of_energy(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1058, 0, self_id).await
    }

    /// 设置在线状态为"宝宝认证"
    async fn set_baby_certified(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1070, 0, self_id).await
    }

    /// 设置在线状态为"一言难尽"
    async fn set_hard_to_describe(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1063, 0, self_id).await
    }

    /// 设置在线状态为"难得糊涂"
    async fn set_rare_confusion(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2001, 0, self_id).await
    }

    /// 设置在线状态为"emo中"
    async fn set_emo(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1401, 0, self_id).await
    }

    /// 设置在线状态为"我太难了"
    async fn set_too_hard(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1062, 0, self_id).await
    }

    /// 设置在线状态为"我想开了"
    async fn set_i_understand(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2013, 0, self_id).await
    }

    /// 设置在线状态为"我没事"
    async fn set_im_ok(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1052, 0, self_id).await
    }

    /// 设置在线状态为"想静静"
    async fn set_want_quiet(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1061, 0, self_id).await
    }

    /// 设置在线状态为"悠哉哉"
    async fn set_leisurely(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1059, 0, self_id).await
    }

    /// 设置在线状态为"去旅行"
    async fn set_go_travel(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2015, 0, self_id).await
    }

    /// 设置在线状态为"信号弱"
    async fn set_weak_signal(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1011, 0, self_id).await
    }

    /// 设置在线状态为"出去浪"
    async fn set_go_out(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2003, 0, self_id).await
    }

    /// 设置在线状态为"肝作业"
    async fn set_doing_homework(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2012, 0, self_id).await
    }

    /// 设置在线状态为"学习中"
    async fn set_studying(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1018, 0, self_id).await
    }

    /// 设置在线状态为"搬砖中"
    async fn set_working(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 2023, 0, self_id).await
    }

    /// 设置在线状态为"摸鱼中"
    async fn set_slacking(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1300, 0, self_id).await
    }

    /// 设置在线状态为"无聊中"
    async fn set_bored(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1060, 0, self_id).await
    }

    /// 设置在线状态为"timi中"
    async fn set_timi(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1027, 0, self_id).await
    }

    /// 设置在线状态为"睡觉中"
    async fn set_sleeping(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1016, 0, self_id).await
    }

    /// 设置在线状态为"熬夜中"
    async fn set_staying_up(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1032, 0, self_id).await
    }

    /// 设置在线状态为"追剧中"
    async fn set_watching_drama(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1021, 0, self_id).await
    }

    /// 设置在线状态为"我的电量"
    async fn set_my_battery(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.set_online_status(10, 1000, 0, self_id).await
    }

    /// 获取好友分组列表
    /// 返回包含分组信息及每个分组中的好友列表
    async fn get_friends_with_category(&self, self_id: Option<&str>) -> AyjxResult<Vec<Value>> {
        let resp = self
            .call_api("get_friends_with_category", json!({}), self_id)
            .await?;
        if let Some(array) = resp.as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 设置头像
    /// file: 图片路径或链接，支持本地路径、网络URL、base64或DataURL
    async fn set_qq_avatar(&self, file: &str, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api("set_qq_avatar", json!({ "file": file }), self_id)
            .await?;
        Ok(())
    }

    /// 点赞
    /// user_id: 用户QQ号
    /// times: 点赞次数，默认1
    async fn send_like(
        &self,
        user_id: &str,
        times: Option<u32>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "user_id": user_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(t) = times {
            params["times"] = json!(t);
        }

        self.call_api("send_like", params, self_id).await?;
        Ok(())
    }

    /// 设置私聊已读
    /// user_id: 用户QQ号
    async fn mark_private_msg_as_read(
        &self,
        user_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "mark_private_msg_as_read",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 设置群聊已读
    /// group_id: 群号
    async fn mark_group_msg_as_read(
        &self,
        group_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "mark_group_msg_as_read",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 创建收藏
    /// raw_data: 内容
    /// brief: 标题
    async fn create_collection(
        &self,
        raw_data: &str,
        brief: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "create_collection",
            json!({
                "rawData": raw_data,
                "brief": brief
            }),
            self_id,
        )
        .await
    }

    /// 处理好友请求
    /// flag: 请求标识符
    /// approve: 是否同意
    /// remark: 好友备注
    async fn set_friend_add_request(
        &self,
        flag: &str,
        approve: bool,
        remark: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_friend_add_request",
            json!({
                "flag": flag,
                "approve": approve,
                "remark": remark
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 设置个性签名
    /// long_nick: 签名内容
    async fn set_self_longnick(&self, long_nick: &str, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api(
            "set_self_longnick",
            json!({
                "longNick": long_nick
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取登录号信息
    /// 返回当前登录账号的 user_id 和 nickname
    async fn get_login_info(&self, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("get_login_info", json!({}), self_id).await
    }

    /// 获取最近消息列表
    /// count: 会话数量，默认10
    async fn get_recent_contact(
        &self,
        count: Option<u32>,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<Value>> {
        let mut params = json!({});
        if let Some(c) = count {
            params["count"] = json!(c);
        }
        let resp = self.call_api("get_recent_contact", params, self_id).await?;
        if let Some(array) = resp.as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 获取账号信息
    /// user_id: 用户QQ号
    /// 返回包含用户详细信息，如昵称、年龄、性别、个性签名、注册时间、会员状态等
    async fn get_stranger_info(&self, user_id: &str, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api(
            "get_stranger_info",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await
    }

    /// 获取好友列表
    /// no_cache: 是否不使用缓存，默认false
    async fn get_friend_list(
        &self,
        no_cache: Option<bool>,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<Value>> {
        let mut params = json!({});
        if let Some(cache) = no_cache {
            params["no_cache"] = json!(cache);
        }
        let resp = self.call_api("get_friend_list", params, self_id).await?;
        if let Some(array) = resp.as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 设置所有消息已读
    async fn mark_all_as_read(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api("_mark_all_as_read", json!({}), self_id)
            .await?;
        Ok(())
    }

    /// 获取点赞列表
    /// user_id: 指定用户，不填为获取所有
    /// start: 起始位置，默认0
    /// count: 获取数量，默认10
    async fn get_profile_like(
        &self,
        user_id: Option<&str>,
        start: Option<u32>,
        count: Option<u32>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({});

        if let Some(uid) = user_id {
            params["user_id"] = json!(uid.parse::<i64>().unwrap_or(0));
        }

        if let Some(s) = start {
            params["start"] = json!(s);
        }

        if let Some(c) = count {
            params["count"] = json!(c);
        }

        self.call_api("get_profile_like", params, self_id).await
    }

    /// 获取收藏表情
    /// count: 获取数量，默认48
    async fn fetch_custom_face(
        &self,
        count: Option<u32>,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<String>> {
        let mut params = json!({});
        if let Some(c) = count {
            params["count"] = json!(c);
        }
        let resp = self.call_api("fetch_custom_face", params, self_id).await?;
        if let Some(array) = resp.as_array() {
            Ok(array
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
        } else {
            Ok(Vec::new())
        }
    }

    /// 删除好友
    /// user_id: 用户QQ号
    /// friend_id: 好友QQ号（通常与 user_id 相同）
    /// temp_block: 是否拉黑
    /// temp_both_del: 是否双向删除
    async fn delete_friend(
        &self,
        user_id: &str,
        friend_id: &str,
        temp_block: bool,
        temp_both_del: bool,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "delete_friend",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "friend_id": friend_id.parse::<i64>().unwrap_or(0),
                "temp_block": temp_block,
                "temp_both_del": temp_both_del
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取在线机型列表
    /// model: 机型标识符，默认为 "napcat"
    /// 返回包含机型信息的数组，每个元素包含 variants 对象，其中有 model_show 和 need_pay 字段
    async fn get_model_show(
        &self,
        model: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<Value>> {
        let mut params = json!({});
        if let Some(m) = model {
            params["model"] = json!(m);
        } else {
            params["model"] = json!("napcat");
        }

        let resp = self.call_api("_get_model_show", params, self_id).await?;
        if let Some(array) = resp.as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 设置在线机型
    /// model: 机型标识符
    /// model_show: 机型显示名称
    async fn set_model_show(
        &self,
        model: &str,
        model_show: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "_set_model_show",
            json!({
                "model": model,
                "model_show": model_show
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取用户状态
    /// user_id: 用户QQ号
    /// 返回包含 status 和 ext_status 的对象
    async fn get_user_status(&self, user_id: &str, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api(
            "nc_get_user_status",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await
    }

    /// 获取状态
    /// 返回包含 online（是否在线）、good（连接是否正常）、stat（统计信息）的对象
    async fn get_status(&self, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("get_status", json!({}), self_id).await
    }

    /// 获取小程序卡片
    /// 支持两种参数格式：
    /// 1. 简化格式（type为bili或weibo）：
    ///    - type: 类型，可选值："bili"（哔哩哔哩）或 "weibo"（微博）
    ///    - title: 标题
    ///    - desc: 描述
    ///    - picUrl: 图片URL
    ///    - jumpUrl: 跳转URL
    ///    - webUrl: 网页URL（可选）
    ///    - rawArkData: 是否返回原始ark数据，默认false
    /// 2. 完整格式：
    ///    - title: 标题
    ///    - desc: 描述
    ///    - picUrl: 图片URL
    ///    - jumpUrl: 跳转URL
    ///    - iconUrl: 图标URL
    ///    - webUrl: 网页URL（可选）
    ///    - appId: 应用ID
    ///    - scene: 场景（数字或字符串）
    ///    - templateType: 模板类型（数字或字符串）
    ///    - businessType: 业务类型（数字或字符串）
    ///    - verType: 版本类型（数字或字符串）
    ///    - shareType: 分享类型（数字或字符串）
    ///    - versionId: 版本ID
    ///    - sdkId: SDK ID
    ///    - withShareTicket: 是否带分享票据（数字或字符串）
    ///    - rawArkData: 是否返回原始ark数据，默认false
    async fn get_mini_app_ark(&self, params: Value, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("get_mini_app_ark", params, self_id).await
    }

    /// 获取单向好友列表
    /// 返回包含单向好友信息的数组，每个元素包含以下字段：
    /// - uin: 用户QQ号
    /// - uid: 用户UID
    /// - nick_name: 昵称
    /// - age: 年龄
    /// - source: 来源
    async fn get_unidirectional_friend_list(
        &self,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<Value>> {
        let resp = self
            .call_api("get_unidirectional_friend_list", json!({}), self_id)
            .await?;
        if let Some(array) = resp.as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 设置自定义在线状态
    /// face_id: 表情ID
    /// face_type: 表情类型
    /// wording: 描述文本
    async fn set_diy_online_status(
        &self,
        face_id: &str,
        face_type: Option<&str>,
        wording: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "face_id": face_id,
        });

        if let Some(ft) = face_type {
            params["face_type"] = json!(ft);
        }

        if let Some(w) = wording {
            params["wording"] = json!(w);
        }

        self.call_api("set_diy_online_status", params, self_id)
            .await?;
        Ok(())
    }

    /// 设置好友备注
    /// user_id: 用户QQ号
    /// remark: 备注名
    async fn set_friend_remark(
        &self,
        user_id: &str,
        remark: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_friend_remark",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "remark": remark
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    // ----- 消息相关 -----
    // 发送群聊消息

    /// 发送群文本
    async fn send_group_msg(
        &self,
        group_id: &str,
        message: Vec<Value>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送群艾特消息
    async fn send_group_at_msg(
        &self,
        group_id: &str,
        qq: &str,
        name: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut message = json!([{
            "type": "at",
            "data": {
                "qq": qq
            }
        }]);

        // 如果提供了 name，添加到 data 中
        if let Some(name_str) = name {
            message[0]["data"]["name"] = json!(name_str);
        }

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送群图片
    async fn send_group_image(
        &self,
        group_id: &str,
        file: &str,
        summary: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut data = json!({
            "file": file
        });
        if let Some(summary_str) = summary {
            data["summary"] = json!(summary_str);
        }

        let message = json!([{
            "type": "image",
            "data": data
        }]);

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送群系统表情
    /// id: 表情 ID，参考 https://bot.q.qq.com/wiki/develop/api-v2/openapi/emoji/model.html#EmojiType
    async fn send_group_face(
        &self,
        group_id: &str,
        id: i64,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "face",
            "data": {
                "id": id
            }
        }]);

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送群 JSON 消息
    async fn send_group_json(
        &self,
        group_id: &str,
        json_data: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "json",
            "data": {
                "data": json_data
            }
        }]);

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送群语音
    /// file: 语音文件路径，支持本地路径 (file://) 或网络 URL
    async fn send_group_record(
        &self,
        group_id: &str,
        file: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "record",
            "data": {
                "file": file
            }
        }]);

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送群视频
    /// file: 视频文件路径，支持本地路径 (file://) 或网络 URL
    async fn send_group_video(
        &self,
        group_id: &str,
        file: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "video",
            "data": {
                "file": file
            }
        }]);

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送群回复消息
    /// id: 要回复的消息 ID
    /// text: 回复的文本内容
    async fn send_group_reply_msg(
        &self,
        group_id: &str,
        id: &str,
        text: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([
            {
                "type": "reply",
                "data": {
                    "id": id.parse::<i64>().unwrap_or(0)
                }
            },
            {
                "type": "text",
                "data": {
                    "text": text
                }
            }
        ]);

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送群音乐卡片
    /// type_: 音乐平台类型，"163" 或 "qq"
    /// id: 对应平台的音乐 ID
    async fn send_group_music(
        &self,
        group_id: &str,
        type_: &str,
        id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "music",
            "data": {
                "type": type_,
                "id": id
            }
        }]);

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送群聊超级表情 - 骰子
    /// result: 骰子点数 (1-6)
    async fn send_group_dice(
        &self,
        group_id: &str,
        result: i32,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "dice",
            "data": {
                "result": result
            }
        }]);

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送群聊超级表情 - 猜拳
    /// result: 猜拳结果 (1=石头, 2=剪刀, 3=布)
    async fn send_group_rps(
        &self,
        group_id: &str,
        result: i32,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "rps",
            "data": {
                "result": result
            }
        }]);

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送群合并转发消息
    /// group_id: 群号
    /// messages: 消息节点列表
    /// news: 外显内容列表
    /// prompt: 外显提示
    /// summary: 底部摘要
    /// source: 内容来源
    #[allow(clippy::too_many_arguments)]
    async fn send_group_forward_msg(
        &self,
        group_id: &str,
        messages: Vec<Value>,
        news: Vec<Value>,
        prompt: &str,
        summary: &str,
        source: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "send_group_forward_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "messages": messages,
                "news": news,
                "prompt": prompt,
                "summary": summary,
                "source": source
            }),
            self_id,
        )
        .await
    }

    /// 发送群文件
    /// file: 文件路径，支持本地路径 (file://) 或网络 URL，或 base64/DataUrl 编码
    /// name: 文件名
    async fn send_group_file(
        &self,
        group_id: &str,
        file: &str,
        name: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "file",
            "data": {
                "file": file,
                "name": name
            }
        }]);

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 消息转发到群
    /// group_id: 目标群号
    /// message_id: 要转发的消息 ID
    async fn forward_group_single_msg(
        &self,
        group_id: &str,
        message_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "forward_group_single_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message_id": message_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 发送群聊戳一戳
    /// group_id: 群号
    /// user_id: 要戳的用户QQ号
    async fn send_group_poke(
        &self,
        group_id: &str,
        user_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "group_poke",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "user_id": user_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 发送群聊自定义音乐卡片
    /// url: 点击卡片跳转的链接
    /// audio: 音频链接
    /// title: 卡片标题
    /// image: 卡片图片链接
    async fn send_group_custom_music(
        &self,
        group_id: &str,
        url: &str,
        audio: &str,
        title: &str,
        image: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "music",
            "data": {
                "type": "custom",
                "url": url,
                "audio": audio,
                "title": title,
                "image": image
            }
        }]);

        self.call_api(
            "send_group_msg",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    // 发送私聊消息
    /// 发送私聊文本
    async fn send_private_msg(
        &self,
        user_id: &str,
        message: Vec<Value>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊图片
    /// file: 图片文件路径，支持本地路径 (file://) 或网络 URL，或 base64 编码
    /// summary: 外显描述，默认为 "[图片]"
    async fn send_private_image(
        &self,
        user_id: &str,
        file: &str,
        summary: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut data = json!({
            "file": file
        });
        if let Some(summary_str) = summary {
            data["summary"] = json!(summary_str);
        }

        let message = json!([{
            "type": "image",
            "data": data
        }]);

        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊系统表情
    /// id: 表情 ID，参考 https://bot.q.qq.com/wiki/develop/api-v2/openapi/emoji/model.html#EmojiType
    async fn send_private_face(
        &self,
        user_id: &str,
        id: i64,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "face",
            "data": {
                "id": id
            }
        }]);

        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊 JSON 消息
    /// json_data: JSON 字符串
    async fn send_private_json(
        &self,
        user_id: &str,
        json_data: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "json",
            "data": {
                "data": json_data
            }
        }]);

        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊语音
    /// file: 语音文件路径，支持本地路径 (file://) 或网络 URL
    async fn send_private_record(
        &self,
        user_id: &str,
        file: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "record",
            "data": {
                "file": file
            }
        }]);

        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊视频
    /// file: 视频文件路径，支持本地路径 (file://) 或网络 URL
    async fn send_private_video(
        &self,
        user_id: &str,
        file: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "video",
            "data": {
                "file": file
            }
        }]);

        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊回复消息
    /// user_id: 用户QQ号
    /// id: 要回复的消息ID
    /// text: 回复的文本内容
    async fn send_private_reply_msg(
        &self,
        user_id: &str,
        id: &str,
        text: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([
            {
                "type": "reply",
                "data": {
                    "id": id.parse::<i64>().unwrap_or(0)
                }
            },
            {
                "type": "text",
                "data": {
                    "text": text
                }
            }
        ]);

        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊音乐卡片
    /// type_: 音乐平台类型，"163" 或 "qq"
    /// id: 对应平台的音乐 ID
    async fn send_private_music(
        &self,
        user_id: &str,
        type_: &str,
        id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "music",
            "data": {
                "type": type_,
                "id": id
            }
        }]);

        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊自定义音乐卡片
    /// url: 点击卡片跳转的链接
    /// audio: 音频链接
    /// title: 卡片标题
    /// image: 卡片图片链接
    async fn send_private_custom_music(
        &self,
        user_id: &str,
        url: &str,
        audio: &str,
        title: &str,
        image: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "music",
            "data": {
                "type": "custom",
                "url": url,
                "audio": audio,
                "title": title,
                "image": image
            }
        }]);

        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊超级表情 - 骰子
    /// result: 骰子点数 (1-6)
    async fn send_private_dice(
        &self,
        user_id: &str,
        result: i32,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "dice",
            "data": {
                "result": result
            }
        }]);

        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊超级表情 - 猜拳
    /// result: 猜拳结果 (1=石头, 2=剪刀, 3=布)
    async fn send_private_rps(
        &self,
        user_id: &str,
        result: i32,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "rps",
            "data": {
                "result": result
            }
        }]);

        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊合并转发消息
    /// user_id: 用户QQ号
    /// messages: 消息节点列表，每个节点包含 nickname、user_id 和 content
    async fn send_private_forward_msg(
        &self,
        user_id: &str,
        messages: Vec<Value>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "send_private_forward_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "messages": messages
            }),
            self_id,
        )
        .await
    }

    /// 消息转发到私聊
    /// user_id: 目标用户QQ号
    /// message_id: 要转发的消息 ID
    async fn forward_private_single_msg(
        &self,
        user_id: &str,
        message_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "forward_private_single_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message_id": message_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 发送私聊文件
    /// file: 文件路径，支持本地路径 (file://) 或网络 URL，或 base64/DataUrl 编码
    /// name: 文件名
    async fn send_private_file(
        &self,
        user_id: &str,
        file: &str,
        name: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let message = json!([{
            "type": "file",
            "data": {
                "file": file,
                "name": name
            }
        }]);

        self.call_api(
            "send_private_msg",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "message": message,
            }),
            self_id,
        )
        .await
    }

    /// 发送私聊戳一戳
    /// user_id: 要戳的用户QQ号
    async fn send_private_poke(&self, user_id: &str, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api(
            "private_poke",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 发送戳一戳
    /// user_id: 被戳的用户QQ号
    /// group_id: 群号（可选，不填则为私聊戳）
    /// target_id: 戳一戳对象（可选）
    async fn send_poke(
        &self,
        user_id: &str,
        group_id: Option<&str>,
        target_id: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "user_id": user_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(gid) = group_id {
            params["group_id"] = json!(gid.parse::<i64>().unwrap_or(0));
        }

        if let Some(tid) = target_id {
            params["target_id"] = json!(tid.parse::<i64>().unwrap_or(0));
        }

        self.call_api("send_poke", params, self_id).await?;
        Ok(())
    }

    /// 撤回消息
    async fn delete_msg(&self, message_id: &str, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api(
            "delete_msg",
            json!({
                "message_id": message_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取群历史消息
    /// group_id: 群号
    /// message_seq: 起始消息序号，0为最新
    /// count: 获取数量，默认20
    /// reverse_order: 是否倒序，默认false
    async fn get_group_msg_history(
        &self,
        group_id: &str,
        message_seq: Option<&str>,
        count: Option<u32>,
        reverse_order: Option<bool>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(seq) = message_seq {
            params["message_seq"] = json!(seq.parse::<i64>().unwrap_or(0));
        }

        if let Some(c) = count {
            params["count"] = json!(c);
        }

        if let Some(rev) = reverse_order {
            params["reverseOrder"] = json!(rev);
        }

        self.call_api("get_group_msg_history", params, self_id)
            .await
    }

    /// 获取消息详情
    async fn get_msg(&self, message_id: &str, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api(
            "get_msg",
            json!({
                "message_id": message_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await
    }

    /// 获取合并转发消息
    /// message_id: 合并转发消息的 ID
    async fn get_forward_msg(&self, message_id: &str, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api(
            "get_forward_msg",
            json!({
                "message_id": message_id
            }),
            self_id,
        )
        .await
    }

    /// 贴表情
    /// message_id: 消息 ID
    /// emoji_id: 表情 ID
    /// set: 是否贴 (true=贴, false=取消)
    async fn set_msg_emoji_like(
        &self,
        message_id: &str,
        emoji_id: i64,
        set: bool,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_msg_emoji_like",
            json!({
                "message_id": message_id.parse::<i64>().unwrap_or(0),
                "emoji_id": emoji_id,
                "set": set
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取好友历史消息
    /// user_id: 好友QQ号
    /// message_seq: 起始消息序号，0为最新
    /// count: 获取数量，默认20
    /// reverse_order: 是否倒序，默认false
    async fn get_friend_msg_history(
        &self,
        user_id: &str,
        message_seq: Option<&str>,
        count: Option<u32>,
        reverse_order: Option<bool>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({
            "user_id": user_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(seq) = message_seq {
            params["message_seq"] = json!(seq.parse::<i64>().unwrap_or(0));
        }

        if let Some(c) = count {
            params["count"] = json!(c);
        }

        if let Some(rev) = reverse_order {
            params["reverseOrder"] = json!(rev);
        }

        self.call_api("get_friend_msg_history", params, self_id)
            .await
    }

    /// 获取贴表情详情
    /// message_id: 消息 ID
    /// emoji_id: 表情 ID
    /// emoji_type: 表情类型
    /// count: 获取数量，默认20
    async fn fetch_emoji_like(
        &self,
        message_id: &str,
        emoji_id: &str,
        emoji_type: &str,
        count: Option<u32>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({
            "message_id": message_id.parse::<i64>().unwrap_or(0),
            "emojiId": emoji_id,
            "emojiType": emoji_type,
        });

        if let Some(c) = count {
            params["count"] = json!(c);
        }

        self.call_api("fetch_emoji_like", params, self_id).await
    }

    /// 发送合并转发消息
    /// group_id: 群号（可选，与 user_id 二选一）
    /// user_id: 用户QQ号（可选，与 group_id 二选一）
    /// messages: 消息节点列表，每个节点包含 type: "node" 和 data
    /// news: 外显内容列表，每个元素包含 text 字段
    /// prompt: 外显提示
    /// summary: 底部摘要
    /// source: 内容来源
    #[allow(clippy::too_many_arguments)]
    async fn send_forward_msg(
        &self,
        group_id: Option<&str>,
        user_id: Option<&str>,
        messages: Vec<Value>,
        news: Vec<Value>,
        prompt: &str,
        summary: &str,
        source: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({
            "messages": messages,
            "news": news,
            "prompt": prompt,
            "summary": summary,
            "source": source
        });

        if let Some(gid) = group_id {
            params["group_id"] = json!(gid.parse::<i64>().unwrap_or(0));
        }

        if let Some(uid) = user_id {
            params["user_id"] = json!(uid.parse::<i64>().unwrap_or(0));
        }

        self.call_api("send_forward_msg", params, self_id).await
    }

    /// 获取语音消息详情
    /// file: 语音文件路径，支持本地路径 (file://) 或网络 URL
    /// file_id: 语音文件 ID
    /// out_format: 输出格式，可选值：mp3, amr, wma, m4a, spx, ogg, wav, flac
    async fn get_record(
        &self,
        file: Option<&str>,
        file_id: Option<&str>,
        out_format: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({
            "out_format": out_format,
        });

        if let Some(f) = file {
            params["file"] = json!(f);
        }

        if let Some(fid) = file_id {
            params["file_id"] = json!(fid);
        }

        self.call_api("get_record", params, self_id).await
    }

    /// 获取图片消息详情
    /// file_id: 文件 ID (与 file 参数二选一)
    /// file: 文件路径 (与 file_id 参数二选一)
    async fn get_image(
        &self,
        file_id: Option<&str>,
        file: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({});

        if let Some(fid) = file_id {
            params["file_id"] = json!(fid);
        }

        if let Some(f) = file {
            params["file"] = json!(f);
        }

        self.call_api("get_image", params, self_id).await
    }

    /// 发送群 AI 语音
    /// group_id: 群号
    /// character: character_id
    /// text: 文本
    async fn send_group_ai_record(
        &self,
        group_id: &str,
        character: &str,
        text: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "send_group_ai_record",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "character": character,
                "text": text
            }),
            self_id,
        )
        .await
    }

    // ----- 群聊相关 -----
    /// 设置群搜索
    /// group_id: 群号
    /// no_code_finger_open: 未知参数，通常为数字
    /// no_finger_open: 未知参数，通常为数字
    async fn set_group_search(
        &self,
        group_id: &str,
        no_code_finger_open: Option<i32>,
        no_finger_open: Option<i32>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(code_finger) = no_code_finger_open {
            params["no_code_finger_open"] = json!(code_finger);
        }

        if let Some(finger) = no_finger_open {
            params["no_finger_open"] = json!(finger);
        }

        self.call_api("set_group_search", params, self_id).await?;
        Ok(())
    }

    /// 获取群详细信息
    /// group_id: 群号
    /// 返回包含群详细信息，如群名称、群备注、成员数量、最大成员数量、是否全体禁言等
    async fn get_group_detail_info(
        &self,
        group_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "get_group_detail_info",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await
    }

    /// 设置群添加选项
    /// group_id: 群号
    /// add_type: 加群方式，可选值："1"（直接加入），"2"（需要管理员同意），"3"（需要回答问题），"4"（需要正确回答问题并由管理员同意）
    /// group_question: 加群问题（当 add_type 为 "3" 或 "4" 时有效）
    /// group_answer: 加群答案（当 add_type 为 "3" 或 "4" 时有效）
    async fn set_group_add_option(
        &self,
        group_id: &str,
        add_type: &str,
        group_question: Option<&str>,
        group_answer: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
            "add_type": add_type,
        });

        if let Some(question) = group_question {
            params["group_question"] = json!(question);
        }

        if let Some(answer) = group_answer {
            params["group_answer"] = json!(answer);
        }

        self.call_api("set_group_add_option", params, self_id)
            .await?;
        Ok(())
    }

    /// 设置群机器人添加选项
    /// group_id: 群号
    /// robot_member_switch: 机器人成员开关
    /// robot_member_examine: 机器人成员审核
    async fn set_group_robot_add_option(
        &self,
        group_id: &str,
        robot_member_switch: Option<i32>,
        robot_member_examine: Option<i32>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(switch) = robot_member_switch {
            params["robot_member_switch"] = json!(switch);
        }

        if let Some(examine) = robot_member_examine {
            params["robot_member_examine"] = json!(examine);
        }

        self.call_api("set_group_robot_add_option", params, self_id)
            .await?;
        Ok(())
    }

    /// 批量踢出群成员
    /// group_id: 群号
    /// user_id: 用户QQ号列表
    /// reject_add_request: 是否群拉黑
    async fn set_group_kick_members(
        &self,
        group_id: &str,
        user_id: Vec<&str>,
        reject_add_request: Option<bool>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
            "user_id": user_id
                .iter()
                .map(|uid| uid.parse::<i64>().unwrap_or(0))
                .collect::<Vec<i64>>(),
        });

        if let Some(reject) = reject_add_request {
            params["reject_add_request"] = json!(reject);
        }

        self.call_api("set_group_kick_members", params, self_id)
            .await?;
        Ok(())
    }

    /// 设置群备注
    /// group_id: 群号
    /// remark: 群备注
    async fn set_group_remark(
        &self,
        group_id: &str,
        remark: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_group_remark",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "remark": remark
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 群踢人
    /// group_id: 群号
    /// user_id: 用户QQ号
    /// reject_add_request: 是否群拉黑，默认false
    async fn set_group_kick(
        &self,
        group_id: &str,
        user_id: &str,
        reject_add_request: Option<bool>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
            "user_id": user_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(reject) = reject_add_request {
            params["reject_add_request"] = json!(reject);
        }

        self.call_api("set_group_kick", params, self_id).await?;
        Ok(())
    }

    /// 获取群系统消息
    /// count: 获取数量，默认50
    async fn get_group_system_msg(
        &self,
        count: Option<u32>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({});
        if let Some(c) = count {
            params["count"] = json!(c);
        }
        self.call_api("get_group_system_msg", params, self_id).await
    }

    /// 群禁言
    /// group_id: 群号
    /// user_id: 用户QQ号
    /// duration: 禁言时间（秒），0为取消禁言
    async fn set_group_ban(
        &self,
        group_id: &str,
        user_id: &str,
        duration: u32,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_group_ban",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "duration": duration
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取群精华消息列表
    /// group_id: 群号
    /// 返回包含精华消息信息的数组，每个元素包含以下字段：
    /// - msg_seq: 消息序列号
    /// - msg_random: 消息随机数
    /// - sender_id: 发送人账号
    /// - sender_nick: 发送人昵称
    /// - operator_id: 设精人账号
    /// - operator_nick: 设精人昵称
    /// - message_id: 消息ID
    /// - operator_time: 设精时间
    /// - content: 消息内容数组
    async fn get_essence_msg_list(
        &self,
        group_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<Value>> {
        let resp = self
            .call_api(
                "get_essence_msg_list",
                json!({
                    "group_id": group_id.parse::<i64>().unwrap_or(0)
                }),
                self_id,
            )
            .await?;
        if let Some(array) = resp.as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 全体禁言
    /// group_id: 群号
    /// enable: 是否开启全体禁言
    async fn set_group_whole_ban(
        &self,
        group_id: &str,
        enable: bool,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_group_whole_ban",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "enable": enable
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 设置群头像
    /// group_id: 群号
    /// file: 图片文件路径，支持本地路径 (file://) 或网络 URL
    async fn set_group_portrait(
        &self,
        group_id: &str,
        file: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_group_portrait",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "file": file
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 设置群管理
    /// group_id: 群号
    /// user_id: 用户QQ号
    /// enable: true 设置为管理员，false 取消管理员
    async fn set_group_admin(
        &self,
        group_id: &str,
        user_id: &str,
        enable: bool,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_group_admin",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "enable": enable
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 设置群成员名片
    /// group_id: 群号
    /// user_id: 用户QQ号
    /// card: 群名片，为空则为取消群名片
    async fn set_group_card(
        &self,
        group_id: &str,
        user_id: &str,
        card: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_group_card",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "card": card
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 设置群精华消息
    /// message_id: 消息ID
    async fn set_essence_msg(&self, message_id: &str, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api(
            "set_essence_msg",
            json!({
                "message_id": message_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 设置群名
    /// group_id: 群号
    /// group_name: 新群名
    async fn set_group_name(
        &self,
        group_id: &str,
        group_name: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_group_name",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "group_name": group_name
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 删除群精华消息
    /// message_id: 消息ID
    async fn delete_essence_msg(&self, message_id: &str, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api(
            "delete_essence_msg",
            json!({
                "message_id": message_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 退群
    /// group_id: 群号
    /// is_dismiss: 暂无作用
    async fn set_group_leave(
        &self,
        group_id: &str,
        is_dismiss: Option<bool>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(dismiss) = is_dismiss {
            params["is_dismiss"] = json!(dismiss);
        }

        self.call_api("set_group_leave", params, self_id).await?;
        Ok(())
    }

    /// 发送群公告
    /// group_id: 群号
    /// content: 公告内容
    /// image: 图片路径（可选）
    /// pinned: 是否置顶（可选，数字或字符串）
    /// type: 公告类型（可选，数字或字符串）
    /// confirm_required: 是否需要确认（可选，数字或字符串）
    /// is_show_edit_card: 是否显示编辑卡片（可选，数字或字符串）
    /// tip_window_type: 提示窗口类型（可选，数字或字符串）
    #[allow(clippy::too_many_arguments)]
    async fn send_group_notice(
        &self,
        group_id: &str,
        content: &str,
        image: Option<&str>,
        pinned: Option<&str>,
        type_: Option<&str>,
        confirm_required: Option<&str>,
        is_show_edit_card: Option<&str>,
        tip_window_type: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
            "content": content,
        });

        if let Some(img) = image {
            params["image"] = json!(img);
        }

        if let Some(p) = pinned {
            params["pinned"] = json!(p);
        }

        if let Some(t) = type_ {
            params["type"] = json!(t);
        }

        if let Some(c) = confirm_required {
            params["confirm_required"] = json!(c);
        }

        if let Some(e) = is_show_edit_card {
            params["is_show_edit_card"] = json!(e);
        }

        if let Some(tip) = tip_window_type {
            params["tip_window_type"] = json!(tip);
        }

        self.call_api("_send_group_notice", params, self_id).await?;
        Ok(())
    }

    /// 设置群头衔
    /// group_id: 群号
    /// user_id: 用户QQ号
    /// special_title: 头衔，为空则取消头衔
    async fn set_group_special_title(
        &self,
        group_id: &str,
        user_id: &str,
        special_title: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_group_special_title",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "special_title": special_title
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取群公告
    /// group_id: 群号
    /// 返回包含公告信息的数组，每个元素包含以下字段：
    /// - notice_id: 公告ID
    /// - sender_id: 发送人账号
    /// - publish_time: 发送时间
    /// - message: 消息内容（包含文本和图片信息）
    async fn get_group_notice(
        &self,
        group_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<Value>> {
        let resp = self
            .call_api(
                "_get_group_notice",
                json!({
                    "group_id": group_id.parse::<i64>().unwrap_or(0)
                }),
                self_id,
            )
            .await?;
        if let Some(array) = resp.as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 处理加群请求
    /// flag: 请求标识符（来自加群请求事件）
    /// approve: 是否同意请求
    /// reason: 拒绝理由（仅在拒绝时有效）
    async fn set_group_add_request(
        &self,
        flag: &str,
        approve: bool,
        reason: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "flag": flag,
            "approve": approve,
        });

        if let Some(r) = reason {
            params["reason"] = json!(r);
        }

        self.call_api("set_group_add_request", params, self_id)
            .await?;
        Ok(())
    }

    /// 获取群信息
    /// group_id: 群号
    /// 返回包含群信息的对象，如群名称、群备注、成员数量、最大成员数量、是否全体禁言等
    async fn get_group_info(&self, group_id: &str, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api(
            "get_group_info",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await
    }

    /// 获取群列表
    /// no_cache: 是否不使用缓存，默认false
    /// 返回包含群信息的数组，每个元素包含以下字段：
    /// - group_id: 群号
    /// - group_name: 群名称
    /// - group_memo: 群备注
    /// - group_create_time: 群创建时间
    /// - group_level: 群等级
    /// - member_count: 成员数量
    /// - max_member_count: 最大成员数量
    /// - group_type: 群类型
    async fn get_group_list(
        &self,
        no_cache: Option<bool>,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<Value>> {
        let mut params = json!({});
        if let Some(cache) = no_cache {
            params["no_cache"] = json!(cache);
        }

        let resp = self.call_api("get_group_list", params, self_id).await?;
        if let Some(array) = resp.as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 删除群公告
    /// group_id: 群号
    /// notice_id: 公告ID
    async fn delete_group_notice(
        &self,
        group_id: &str,
        notice_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "_del_group_notice",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "notice_id": notice_id
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取群成员信息
    /// group_id: 群号
    /// user_id: 用户QQ号
    /// no_cache: 是否不使用缓存，默认false
    /// 返回包含群成员信息的对象，如用户ID、昵称、群名片、角色、加入时间、最后发言时间等
    async fn get_group_member_info(
        &self,
        group_id: &str,
        user_id: &str,
        no_cache: Option<bool>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
            "user_id": user_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(cache) = no_cache {
            params["no_cache"] = json!(cache);
        }

        self.call_api("get_group_member_info", params, self_id)
            .await
    }

    /// 获取群成员列表
    /// group_id: 群号
    /// no_cache: 是否不使用缓存，默认false
    /// 返回包含群成员信息的数组，每个元素包含以下字段：
    /// - user_id: 用户QQ号
    /// - nickname: 昵称
    /// - card: 群名片
    /// - sex: 性别
    /// - age: 年龄
    /// - area: 地区
    /// - join_time: 加入时间
    /// - last_sent_time: 最后发言时间
    /// - level: 成员等级
    /// - role: 角色（owner/admin/member）
    /// - unfriendly: 是否不良记录成员
    /// - title: 专属头衔
    /// - title_expire_time: 头衔过期时间
    /// - card_changeable: 是否允许修改群名片
    /// - shut_up_timestamp: 禁言到期时间
    async fn get_group_member_list(
        &self,
        group_id: &str,
        no_cache: Option<bool>,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<Value>> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(cache) = no_cache {
            params["no_cache"] = json!(cache);
        }

        let resp = self
            .call_api("get_group_member_list", params, self_id)
            .await?;
        if let Some(array) = resp.as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 获取群荣誉
    /// group_id: 群号
    /// type_: 荣誉类型，可选值："all"(默认), "talkative"(群聊之火), "performer"(群聊炽焰), "legend"(龙王), "strong_newbie"(冒尖小春笋), "emotion"(快乐源泉)
    async fn get_group_honor_info(
        &self,
        group_id: &str,
        type_: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(t) = type_ {
            params["type"] = json!(t);
        }

        self.call_api("get_group_honor_info", params, self_id).await
    }

    /// 获取群信息ex
    /// group_id: 群号
    async fn get_group_info_ex(&self, group_id: &str, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api(
            "get_group_info_ex",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await
    }

    /// 获取群 @全体成员 剩余次数
    /// group_id: 群号
    /// 返回包含以下字段的对象：
    /// - can_at_all: 是否可以 @全体成员
    /// - remain_at_all_count_for_group: 群内剩余 @全体成员 次数
    /// - remain_at_all_count_for_uin: 机器人账号剩余 @全体成员 次数
    async fn get_group_at_all_remain(
        &self,
        group_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "get_group_at_all_remain",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await
    }

    /// 获取群禁言列表
    /// group_id: 群号
    /// 返回包含禁言成员信息的数组，每个元素包含以下字段：
    /// - uid: 用户ID
    /// - qid: 用户QQ号
    /// - uin: 用户UIN
    /// - nick: 昵称
    /// - remark: 备注
    /// - cardType: 名片类型
    /// - cardName: 名片名称
    /// - role: 角色
    /// - avatarPath: 头像路径
    /// - shutUpTime: 解禁时间（时间戳）
    /// - isDelete: 是否已删除
    /// - isSpecialConcerned: 是否特别关心
    /// - isSpecialShield: 是否特别屏蔽
    /// - isRobot: 是否为机器人
    /// - groupHonor: 群荣誉信息
    /// - memberRealLevel: 群聊等级
    /// - memberLevel: 成员等级
    /// - globalGroupLevel: 全局群等级
    /// - globalGroupPoint: 全局群积分
    /// - memberTitleId: 成员头衔ID
    /// - memberSpecialTitle: 成员特殊头衔
    /// - specialTitleExpireTime: 特殊头衔过期时间
    /// - userShowFlag: 用户显示标志
    /// - userShowFlagNew: 用户显示标志（新）
    /// - richFlag: 财富标志
    /// - mssVipType: MSS会员类型
    /// - bigClubLevel: 大会员等级
    /// - bigClubFlag: 大会员标志
    /// - autoRemark: 自动备注
    /// - creditLevel: 信用等级
    /// - joinTime: 入群时间
    /// - lastSpeakTime: 最后发言时间
    /// - memberFlag: 成员标志
    /// - memberFlagExt: 成员扩展标志
    /// - memberMobileFlag: 成员手机标志
    /// - memberFlagExt2: 成员扩展标志2
    /// - isSpecialShielded: 是否被特别屏蔽
    /// - cardNameId: 名片名称ID
    async fn get_group_shut_list(
        &self,
        group_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<Value>> {
        let resp = self
            .call_api(
                "get_group_shut_list",
                json!({
                    "group_id": group_id.parse::<i64>().unwrap_or(0)
                }),
                self_id,
            )
            .await?;
        if let Some(array) = resp["data"].as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 获取群过滤系统消息
    /// 返回包含两个数组的对象：
    /// - InvitedRequest: 被过滤的群邀请请求列表
    /// - join_requests: 被过滤的加群请求列表
    ///
    /// 每个请求包含以下字段：
    /// - request_id: 请求ID
    /// - invitor_uin: 邀请人QQ号（仅InvitedRequest）
    /// - invitor_nick: 邀请人昵称（仅InvitedRequest）
    /// - group_id: 群号
    /// - message: 请求消息
    /// - group_name: 群名称
    /// - checked: 是否已处理
    /// - actor: 操作人QQ号
    /// - requester_nick: 请求人昵称（仅join_requests）
    async fn get_group_ignored_notifies(&self, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("get_group_ignored_notifies", json!({}), self_id)
            .await
    }

    /// 群打卡
    async fn set_group_sign(&self, group_id: &str, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api(
            "set_group_sign",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 群打卡 (别名接口)
    /// group_id: 群号
    async fn send_group_sign(&self, group_id: &str, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api(
            "send_group_sign",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取被过滤的加群请求
    async fn get_group_ignore_add_request(&self, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("get_group_ignore_add_request", json!({}), self_id)
            .await
    }

    // ----- 文件相关 ----- //
    /// 移动群文件
    /// group_id: 群号
    /// file_id: 文件 ID
    /// current_parent_directory: 当前父目录（根目录填 "/"）
    /// target_parent_directory: 目标父目录
    async fn move_group_file(
        &self,
        group_id: &str,
        file_id: &str,
        current_parent_directory: &str,
        target_parent_directory: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "move_group_file",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "file_id": file_id,
                "current_parent_directory": current_parent_directory,
                "target_parent_directory": target_parent_directory
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 转存为永久文件
    /// group_id: 群号
    /// file_id: 文件 ID
    async fn trans_group_file(
        &self,
        group_id: &str,
        file_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "trans_group_file",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "file_id": file_id
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 重命名群文件
    /// group_id: 群号
    /// file_id: 文件 ID
    /// current_parent_directory: 当前父目录（根目录填 "/"）
    /// new_name: 新文件名
    async fn rename_group_file(
        &self,
        group_id: &str,
        file_id: &str,
        current_parent_directory: &str,
        new_name: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "rename_group_file",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "file_id": file_id,
                "current_parent_directory": current_parent_directory,
                "new_name": new_name
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取文件信息
    /// file_id: 文件 ID (与 file 参数二选一)
    /// file: 文件路径 (与 file_id 参数二选一)
    async fn get_file(
        &self,
        file_id: Option<&str>,
        file: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({});

        if let Some(fid) = file_id {
            params["file_id"] = json!(fid);
        }

        if let Some(f) = file {
            params["file"] = json!(f);
        }

        self.call_api("get_file", params, self_id).await
    }
    /// 上传群文件
    /// group_id: 群号
    /// file: 文件路径
    /// name: 文件名
    /// folder: 文件夹ID（与 folder_id 二选一）
    /// folder_id: 文件夹ID（与 folder 二选一）
    async fn upload_group_file(
        &self,
        group_id: &str,
        file: &str,
        name: &str,
        folder: Option<&str>,
        folder_id: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
            "file": file,
            "name": name
        });

        // 优先使用 folder 参数，如果未提供则使用 folder_id
        if let Some(folder_str) = folder {
            params["folder"] = json!(folder_str);
        } else if let Some(fid) = folder_id {
            params["folder_id"] = json!(fid);
        }

        self.call_api("upload_group_file", params, self_id).await?;
        Ok(())
    }

    /// 创建群文件文件夹
    /// group_id: 群号
    /// folder_name: 文件夹名称
    async fn create_group_file_folder(
        &self,
        group_id: &str,
        folder_name: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "create_group_file_folder",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "folder_name": folder_name
            }),
            self_id,
        )
        .await
    }

    /// 删除群文件
    /// group_id: 群号
    /// file_id: 文件 ID
    async fn delete_group_file(
        &self,
        group_id: &str,
        file_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "delete_group_file",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "file_id": file_id
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 删除群文件夹
    /// group_id: 群号
    /// folder_id: 文件夹 ID
    async fn delete_group_folder(
        &self,
        group_id: &str,
        folder_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "delete_group_folder",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "folder_id": folder_id
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 上传私聊文件
    /// user_id: 用户QQ号
    /// file: 文件路径，支持本地路径 (file://) 或网络 URL
    /// name: 文件名
    async fn upload_private_file(
        &self,
        user_id: &str,
        file: &str,
        name: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "upload_private_file",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "file": file,
                "name": name
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 获取群文件系统信息
    /// group_id: 群号
    /// 返回包含以下字段的对象：
    /// - file_count: 文件总数
    /// - limit_count: 文件上限
    /// - used_space: 已使用空间
    /// - total_space: 空间上限
    async fn get_group_file_system_info(
        &self,
        group_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "get_group_file_system_info",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0)
            }),
            self_id,
        )
        .await
    }

    /// 下载文件到缓存目录
    /// url: 下载地址
    /// base64: 和url二选一，base64编码的文件内容
    /// name: 自定义文件名称
    /// headers: 请求头，可以是字符串或字符串数组
    async fn download_file(
        &self,
        url: Option<&str>,
        base64: Option<&str>,
        name: Option<&str>,
        headers: Option<Value>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({});

        if let Some(u) = url {
            params["url"] = json!(u);
        }

        if let Some(b) = base64 {
            params["base64"] = json!(b);
        }

        if let Some(n) = name {
            params["name"] = json!(n);
        }

        if let Some(h) = headers {
            params["headers"] = h;
        }

        self.call_api("download_file", params, self_id).await
    }

    /// 获取群根目录文件列表
    /// group_id: 群号
    /// file_count: 获取文件数量，默认50
    async fn get_group_root_files(
        &self,
        group_id: &str,
        file_count: Option<u32>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(count) = file_count {
            params["file_count"] = json!(count);
        }

        self.call_api("get_group_root_files", params, self_id).await
    }

    /// 获取群子目录文件列表
    /// group_id: 群号
    /// folder_id: 文件夹 ID (与 folder 参数二选一)
    /// folder: 文件夹路径 (与 folder_id 参数二选一)
    /// file_count: 一次性获取的文件数量，默认50
    async fn get_group_files_by_folder(
        &self,
        group_id: &str,
        folder_id: Option<&str>,
        folder: Option<&str>,
        file_count: Option<u32>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(fid) = folder_id {
            params["folder_id"] = json!(fid);
        }

        if let Some(f) = folder {
            params["folder"] = json!(f);
        }

        if let Some(count) = file_count {
            params["file_count"] = json!(count);
        }

        self.call_api("get_group_files_by_folder", params, self_id)
            .await
    }

    /// 获取群文件链接
    /// group_id: 群号
    /// file_id: 文件 ID
    async fn get_group_file_url(
        &self,
        group_id: &str,
        file_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<String> {
        let resp = self
            .call_api(
                "get_group_file_url",
                json!({
                    "group_id": group_id.parse::<i64>().unwrap_or(0),
                    "file_id": file_id
                }),
                self_id,
            )
            .await?;
        Ok(resp["url"].as_str().unwrap_or("").to_string())
    }

    /// 获取私聊文件链接
    /// file_id: 文件 ID
    async fn get_private_file_url(
        &self,
        file_id: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<String> {
        let resp = self
            .call_api(
                "get_private_file_url",
                json!({ "file_id": file_id }),
                self_id,
            )
            .await?;
        Ok(resp["url"].as_str().unwrap_or("").to_string())
    }

    /// 清空缓存
    async fn clean_cache(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api("clean_cache", json!({}), self_id).await?;
        Ok(())
    }

    // ----- 密钥相关 -----
    /// 获取 clientkey
    async fn get_clientkey(&self, self_id: Option<&str>) -> AyjxResult<String> {
        let resp = self.call_api("get_clientkey", json!({}), self_id).await?;
        Ok(resp["data"]["clientkey"].as_str().unwrap_or("").to_string())
    }

    /// 获取cookies
    /// domain: 域名
    async fn get_cookies(&self, domain: &str, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api(
            "get_cookies",
            json!({
                "domain": domain
            }),
            self_id,
        )
        .await
    }

    /// 获取 CSRF Token
    async fn get_csrf_token(&self, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("get_csrf_token", json!({}), self_id).await
    }

    /// 获取 QQ 相关接口凭证
    /// domain: 域名
    async fn get_credentials(&self, domain: &str, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api(
            "get_credentials",
            json!({
                "domain": domain
            }),
            self_id,
        )
        .await
    }

    /// 获取 rkey 列表
    /// 返回包含 rkey 信息的数组，每个元素包含以下字段：
    /// - rkey: rkey 值
    /// - ttl: 过期时间
    /// - time: 时间戳
    /// - type: 类型
    async fn nc_get_rkey(&self, self_id: Option<&str>) -> AyjxResult<Vec<Value>> {
        let resp = self.call_api("nc_get_rkey", json!({}), self_id).await?;
        if let Some(array) = resp["data"].as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 获取 rkey 列表
    /// 返回包含 rkey 信息的数组，每个元素包含以下字段：
    /// - type: 类型
    /// - rkey: rkey 值
    /// - created_at: 创建时间戳
    /// - ttl: 过期时间
    async fn get_rkey_list(&self, self_id: Option<&str>) -> AyjxResult<Vec<Value>> {
        let resp = self.call_api("get_rkey", json!({}), self_id).await?;
        if let Some(array) = resp["data"].as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 获取rkey服务
    /// 返回包含以下字段的对象：
    /// - private_rkey: 私聊rkey
    /// - group_rkey: 群聊rkey
    /// - expired_time: 过期时间
    /// - name: 名称
    async fn get_rkey_server(&self, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("get_rkey_server", json!({}), self_id).await
    }

    // ----- 个人操作 -----
    /// OCR 图片识别
    /// image: 图片路径，支持本地路径 (file://) 或网络 URL
    /// 返回包含 OCR 识别结果的数组，每个元素包含以下字段：
    /// - text: 该行文本总和
    /// - pt1, pt2, pt3, pt4: 顶点坐标 (x, y)
    /// - charBox: 字符框数组，每个元素包含 charText 和 charBox (包含四个顶点坐标)
    /// - score: 置信度分数
    async fn ocr_image(&self, image: &str, self_id: Option<&str>) -> AyjxResult<Vec<Value>> {
        let resp = self
            .call_api(
                "ocr_image",
                json!({
                    "image": image
                }),
                self_id,
            )
            .await?;
        if let Some(array) = resp["data"].as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// .OCR 图片识别
    /// image: 图片路径，支持本地路径 (file://) 或网络 URL
    /// 返回包含 OCR 识别结果的数组，每个元素包含以下字段：
    /// - text: 该行文本总和
    /// - pt1, pt2, pt3, pt4: 顶点坐标 (x, y)
    /// - charBox: 字符框数组，每个元素包含 charText 和 charBox (包含四个顶点坐标)
    /// - score: 置信度分数
    async fn dot_ocr_image(&self, image: &str, self_id: Option<&str>) -> AyjxResult<Vec<Value>> {
        let resp = self
            .call_api(
                ".ocr_image",
                json!({
                    "image": image
                }),
                self_id,
            )
            .await?;
        if let Some(array) = resp["data"].as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 英译中
    /// words: 英文数组
    async fn translate_en2zh(
        &self,
        words: Vec<String>,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<String>> {
        let resp = self
            .call_api("translate_en2zh", json!({ "words": words }), self_id)
            .await?;
        if let Some(array) = resp["data"].as_array() {
            Ok(array
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect())
        } else {
            Ok(Vec::new())
        }
    }

    /// 设置输入状态
    /// user_id: 用户QQ号
    /// event_type: 0(取消/对方正在说话), 1(对方正在输入)
    async fn set_input_status(
        &self,
        user_id: &str,
        event_type: i32,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            "set_input_status",
            json!({
                "user_id": user_id.parse::<i64>().unwrap_or(0),
                "event_type": event_type
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// .对事件执行快速操作
    /// context: 事件数据对象
    /// operation: 快速操作对象
    async fn dot_handle_quick_operation(
        &self,
        context: Value,
        operation: Value,
        self_id: Option<&str>,
    ) -> AyjxResult<()> {
        self.call_api(
            ".handle_quick_operation",
            json!({
                "context": context,
                "operation": operation
            }),
            self_id,
        )
        .await?;
        Ok(())
    }

    /// 检查是否可以发送图片
    async fn can_send_image(&self, self_id: Option<&str>) -> AyjxResult<bool> {
        let resp = self.call_api("can_send_image", json!({}), self_id).await?;
        Ok(resp["data"]["yes"].as_bool().unwrap_or(false))
    }

    /// 检查是否可以发送语音
    async fn can_send_record(&self, self_id: Option<&str>) -> AyjxResult<bool> {
        let resp = self.call_api("can_send_record", json!({}), self_id).await?;
        Ok(resp["data"]["yes"].as_bool().unwrap_or(false))
    }

    /// 获取 AI 语音人物
    /// group_id: 群号
    /// chat_type: 聊天类型，1 或 2
    async fn get_ai_characters(
        &self,
        group_id: &str,
        chat_type: Option<&str>,
        self_id: Option<&str>,
    ) -> AyjxResult<Vec<Value>> {
        let mut params = json!({
            "group_id": group_id.parse::<i64>().unwrap_or(0),
        });

        if let Some(ct) = chat_type {
            params["chat_type"] = json!(ct);
        }

        let resp = self.call_api("get_ai_characters", params, self_id).await?;
        if let Some(array) = resp["data"].as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 点击按钮
    /// group_id: 群号
    /// bot_appid: 机器人 appid
    /// button_id: 按钮 ID
    /// callback_data: 回调数据
    /// msg_seq: 消息序列号
    async fn click_inline_keyboard_button(
        &self,
        group_id: &str,
        bot_appid: &str,
        button_id: &str,
        callback_data: &str,
        msg_seq: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "click_inline_keyboard_button",
            json!({
                "group_id": group_id.parse::<i64>().unwrap_or(0),
                "bot_appid": bot_appid,
                "button_id": button_id,
                "callback_data": callback_data,
                "msg_seq": msg_seq
            }),
            self_id,
        )
        .await
    }

    /// 获取 AI 录音
    /// group_id: 群号
    /// character: character_id
    /// text: 文本
    async fn get_ai_record(
        &self,
        group_id: &str,
        character: &str,
        text: &str,
        self_id: Option<&str>,
    ) -> AyjxResult<String> {
        let resp = self
            .call_api(
                "get_ai_record",
                json!({
                    "group_id": group_id.parse::<i64>().unwrap_or(0),
                    "character": character,
                    "text": text
                }),
                self_id,
            )
            .await?;
        // 根据 json 描述，返回的 data 直接是 string 链接
        if let Some(url) = resp.as_str() {
            Ok(url.to_string())
        } else {
            Ok(resp.to_string())
        }
    }

    // ----- 系统操作 -----
    /// 获取机器人账号范围
    /// 返回包含账号范围信息的数组，每个元素包含以下字段：
    /// - minUin: 最小账号
    /// - maxUin: 最大账号
    async fn get_robot_uin_range(&self, self_id: Option<&str>) -> AyjxResult<Vec<Value>> {
        let resp = self
            .call_api("get_robot_uin_range", json!({}), self_id)
            .await?;
        if let Some(array) = resp["data"].as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// 账号退出
    async fn bot_exit(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api("bot_exit", json!({}), self_id).await?;
        Ok(())
    }

    /// 发送自定义组包
    async fn send_packet(&self, self_id: Option<&str>) -> AyjxResult<()> {
        self.call_api("send_packet", json!({}), self_id).await?;
        Ok(())
    }

    /// 获取packet状态
    async fn nc_get_packet_status(&self, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("nc_get_packet_status", json!({}), self_id)
            .await
    }

    /// 获取版本信息
    /// 返回包含以下字段的对象：
    /// - app_name: 应用名称
    /// - protocol_version: 协议版本
    /// - app_version: 应用版本
    async fn get_version_info(&self, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("get_version_info", json!({}), self_id).await
    }

    // ----- 其他 -----
    // ----- 保留 -----
    // /// 发送私聊消息
    // /// user_id: 用户QQ号
    // /// message: 消息内容
    //async fn send_private_msg(
    //     &self,
    //     user_id: &str,
    //     message: Vec<Value>,
    //     self_id: Option<&str>,
    // ) -> AyjxResult<Value> {
    //     self.call_api(
    //         "send_private_msg",
    //         json!({
    //             "user_id": user_id.parse::<i64>().unwrap_or(0),
    //             "message": message
    //         }),
    //         self_id,
    //     )
    //     .await
    // }

    // /// 发送群消息
    // /// group_id: 群号
    // /// message: 消息内容
    //async fn send_group_msg(
    //     &self,
    //     group_id: &str,
    //     message: Vec<Value>,
    //     self_id: Option<&str>,
    // ) -> AyjxResult<Value> {
    //     self.call_api(
    //         "send_group_msg",
    //         json!({
    //             "group_id": group_id.parse::<i64>().unwrap_or(0),
    //             "message": message
    //         }),
    //         self_id,
    //     )
    //     .await
    // }

    /// 发送消息
    /// message_type: 消息类型，可选值："private"（私聊），"group"（群聊）
    /// group_id: 群号（当 message_type 为 "group" 时必填）
    /// user_id: 用户QQ号（当 message_type 为 "private" 时必填）
    /// message: 消息内容数组
    async fn send_msg(
        &self,
        message_type: &str,
        group_id: Option<&str>,
        user_id: Option<&str>,
        message: Vec<Value>,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        let mut params = json!({
            "message_type": message_type,
            "message": message
        });

        if let Some(gid) = group_id {
            params["group_id"] = json!(gid.parse::<i64>().unwrap_or(0));
        }

        if let Some(uid) = user_id {
            params["user_id"] = json!(uid.parse::<i64>().unwrap_or(0));
        }

        self.call_api("send_msg", params, self_id).await
    }
    // ----- 接口 -----
    /// unknown
    async fn unknown(&self, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("unknown", json!({}), self_id).await
    }
    /// 获取群列表
    /// 返回包含群信息的数组，每个元素包含以下字段：
    /// - group_id: 群号
    /// - group_name: 群名称
    /// - group_memo: 群备注
    /// - group_create_time: 群创建时间
    /// - group_level: 群等级
    /// - member_count: 成员数量
    /// - max_member_count: 最大成员数量
    /// - group_type: 群类型
    async fn get_guild_list(&self, self_id: Option<&str>) -> AyjxResult<Vec<Value>> {
        let resp = self.call_api("get_guild_list", json!({}), self_id).await?;
        if let Some(array) = resp.as_array() {
            Ok(array.clone())
        } else {
            Ok(Vec::new())
        }
    }
    /// get_guild_service_profile
    async fn get_guild_service_profile(&self, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("get_guild_service_profile", json!({}), self_id)
            .await
    }
    /// 检查链接安全性
    async fn check_url_safely(&self, self_id: Option<&str>) -> AyjxResult<Value> {
        self.call_api("check_url_safely", json!({}), self_id).await
    }
    // ----- bug -----
    /// 获取收藏列表
    /// category: 分类
    /// count: 数量
    async fn get_collection_list(
        &self,
        category: i32,
        count: i32,
        self_id: Option<&str>,
    ) -> AyjxResult<Value> {
        self.call_api(
            "get_collection_list",
            json!({
                "category": category,
                "count": count
            }),
            self_id,
        )
        .await
    }
    // /// 获取被过滤的加群请求
    // /// 返回包含被过滤的加群请求信息的数组，每个元素包含以下字段：
    // /// - request_id: 请求ID
    // /// - invitor_uin: 邀请人QQ号
    // /// - invitor_nick: 邀请人昵称
    // /// - group_id: 群号
    // /// - message: 请求消息
    // /// - group_name: 群名称
    // /// - checked: 是否已处理
    // /// - actor: 操作人QQ号
    // /// - requester_nick: 请求人昵称
    //async fn get_group_ignore_add_request(
    //     &self,
    //     self_id: Option<&str>,
    // ) -> AyjxResult<Vec<Value>> {
    //     let resp = self
    //         .call_api("get_group_ignore_add_request", json!({}), self_id)
    //         .await?;
    //     if let Some(array) = resp["data"].as_array() {
    //         Ok(array.clone())
    //     } else {
    //         Ok(Vec::new())
    //     }
    // }
}
