// 统一聊天面板组件库出口。
//
// 官方 shadcn base-rhea 组件（ui/ 目录源码按官方 Manual 安装 1:1 内嵌，行为来自
// @shadcn/react；两个面板 = 官方 message-scroller-demo 同款骨架）：
//   · 骨架 Card* / Empty* / InputGroup* —— 对话框壳、空状态、输入组。
//   · 滚动框 MessageScroller* —— 两个面板共用的「扫描框」。
//   · 消息 Message*/Bubble* —— 划词追问（QA）的对话形态（带头像）。
//   · 标记 Marker* —— Less Computer 的工具调用 / 上下文压缩标签形态（无头像）。
// 本仓库组装层：ThinkingOrb/OrbAvatar（胶囊同款思考动画）、UserAvatar（GitHub）、
// AssistantMarkdown（流式 markdown）、useChatPanelLifecycle（出现/消失动画信号）。

export { Button, buttonVariants } from './ui/button';
export { Input } from './ui/input';
export { Textarea } from './ui/textarea';
export { Spinner } from './ui/spinner';
export {
  Card,
  CardAction,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from './ui/card';
export {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from './ui/empty';
export {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
  InputGroupText,
  InputGroupTextarea,
} from './ui/input-group';
export {
  MessageScroller,
  MessageScrollerButton,
  MessageScrollerContent,
  MessageScrollerItem,
  MessageScrollerProvider,
  MessageScrollerViewport,
  useMessageScroller,
  useMessageScrollerScrollable,
  useMessageScrollerVisibility,
} from './ui/message-scroller';
export {
  Message,
  MessageAvatar,
  MessageContent,
  MessageFooter,
  MessageGroup,
  MessageHeader,
} from './ui/message';
export { Bubble, BubbleContent, BubbleGroup } from './ui/bubble';
export { Marker, MarkerContent, MarkerIcon, markerVariants } from './ui/marker';
export { ThinkingOrb, OrbAvatar, UserAvatar, useGithubLogin } from './avatars';
export { AssistantMarkdown } from './markdown';
export { useChatPanelLifecycle } from './lifecycle';
