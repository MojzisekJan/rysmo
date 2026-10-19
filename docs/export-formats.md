# Model exports

Use **File → Export** or search for an export command. Export includes visible model geometry, including transformed component occurrences and generated curved surfaces. Hidden objects and objects on hidden tags are omitted. Keep a `.rysmo` file to retain editable model data.

| Format | Contents | Limitations |
| --- | --- | --- |
| OBJ + MTL | Geometry and materials | Separate companion files |
| glTF (.glb) | Geometry, materials and textures | Mesh representation |
| COLLADA (.dae) | Triangulated faces, loose edges, colours, UV coordinates and textures | World-space meshes with flat face normals; no editable component hierarchy, annotations or scenes |
| DXF (.dxf) | Triangulated 3D faces, loose lines and face colours | No textures, annotations or CAD solids |
| STL (.stl) | Binary triangles in millimetres | No colours, one mesh |
| 3MF (.3mf) | Triangles in millimetres, one object per group or component | No colours or textures |

COLLADA uses version 1.4.1, metres and Y-up coordinates. Names encode spaces and punctuation as `_uXXXX_` to meet the format's XML naming rules. Desktop export writes texture images beside the DAE file. A textured web export downloads one ZIP containing the DAE file and its images. Extract them into the same folder before importing. An untextured web export downloads the DAE file directly.

DXF uses the ASCII DXF 2004 format, metres and Z-up coordinates. Faces are `3DFACE` entities, with internal triangulation edges marked invisible. Loose edges are `LINE` entities. Each component occurrence gets a separate layer. Importers that show all mesh edges may still display triangle diagonals.

STL and 3MF are for 3D printing and keep Z up. Before export, Rysmo checks that every object is closed: each edge between exactly two faces that face the same way, and no part inside out. Problems are listed and the edges marked in red; you can still export.

3DS, DWG, FBX and KMZ export are not implemented. Renaming another export does not produce these formats.

The COLLADA output was checked against the [Khronos 1.4.1 schema](https://www.khronos.org/files/collada_schema_1_4_1.xsd). DXF face records follow the [Autodesk 3DFACE reference](https://help.autodesk.com/cloudhelp/2024/ENU/AutoCAD-DXF/files/GUID-747865D5-51F0-45F2-BEFE-9572DBC5B151.htm).
