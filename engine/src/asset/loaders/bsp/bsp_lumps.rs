use sourcerenderer_bsp::{Leaf, Node, LeafFace, LeafBrush, SurfaceEdge, Vertex, Face, Edge, Plane, TextureData, TextureDataStringTable, TextureInfo, TextureStringData, DispInfo, DispVert, DispTri, Lighting, Visibility, Entities};
use sourcerenderer_bsp::game_lumps::StaticPropDict;

pub(super) struct BspLumps {
  pub(super) map_name: String,
  pub(super) leafs: Vec<Leaf>,
  pub(super) nodes: Vec<Node>,
  pub(super) leaf_faces: Vec<LeafFace>,
  pub(super) leaf_brushes: Vec<LeafBrush>,
  pub(super) surface_edges: Vec<SurfaceEdge>,
  pub(super) vertices: Vec<Vertex>,
  pub(super) faces: Vec<Face>,
  pub(super) edges: Vec<Edge>,
  pub(super) planes: Vec<Plane>,
  pub(super) tex_data: Vec<TextureData>,
  pub(super) tex_info: Vec<TextureInfo>,
  pub(super) tex_string_data: TextureStringData,
  pub(super) tex_data_string_table: Vec<TextureDataStringTable>,
  pub(super) disp_infos: Vec<DispInfo>,
  pub(super) disp_verts: Vec<DispVert>,
  pub(super) disp_tris: Vec<DispTri>,
  pub(super) lighting: Vec<Lighting>,
  pub(super) visibility: Visibility,
  pub(super) static_props: StaticPropDict,
  pub(super) entities: Entities,
}
