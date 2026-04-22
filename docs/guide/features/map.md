# 全球地图

原理是读取 agent namespace kv里的几个变量
- metadata_longitude
- metadata_latitude
- metadata_region 

当没有经纬度时会用区域来映射到粗略的经纬度，如果也没有则不显示

会用js worker来定期更新所有节点的经纬度信息