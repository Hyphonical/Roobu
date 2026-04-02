use std::collections::{BTreeMap, HashSet};

use anyhow::ensure;

#[derive(Clone, Copy, Debug)]
pub struct GraphHdbscanParams {
	pub min_cluster_size: usize,
	pub min_samples: usize,
	pub epsilon: f32,
	pub neighbors: usize,
	pub pivots: usize,
	pub top_pivots: usize,
	pub max_candidates: usize,
	pub random_seed: u64,
}

#[derive(Clone, Copy, Debug)]
struct WeightedEdge {
	u: usize,
	v: usize,
	weight: f32,
}

#[derive(Debug)]
struct GraphComponent {
	points: Vec<usize>,
	edges: Vec<WeightedEdge>,
}

#[derive(Clone, Copy, Debug)]
struct DendrogramNode {
	left: Option<usize>,
	right: Option<usize>,
	size: usize,
	merge_distance: f32,
}

#[derive(Debug)]
struct DisjointSet {
	parent: Vec<usize>,
	rank: Vec<u8>,
}

impl DisjointSet {
	fn new(len: usize) -> Self {
		Self {
			parent: (0..len).collect(),
			rank: vec![0; len],
		}
	}

	fn find(&mut self, value: usize) -> usize {
		if self.parent[value] != value {
			let root = self.find(self.parent[value]);
			self.parent[value] = root;
		}
		self.parent[value]
	}

	fn union(&mut self, lhs: usize, rhs: usize) -> usize {
		let mut lhs_root = self.find(lhs);
		let mut rhs_root = self.find(rhs);
		if lhs_root == rhs_root {
			return lhs_root;
		}

		if self.rank[lhs_root] < self.rank[rhs_root] {
			std::mem::swap(&mut lhs_root, &mut rhs_root);
		}

		self.parent[rhs_root] = lhs_root;
		if self.rank[lhs_root] == self.rank[rhs_root] {
			self.rank[lhs_root] += 1;
		}

		lhs_root
	}
}

#[derive(Debug)]
struct KruskalState {
	dsu: DisjointSet,
	set_sizes: Vec<usize>,
	set_nodes: Vec<usize>,
}

impl KruskalState {
	fn new(leaves: usize) -> Self {
		Self {
			dsu: DisjointSet::new(leaves),
			set_sizes: vec![1; leaves],
			set_nodes: (0..leaves).collect(),
		}
	}

	fn find(&mut self, value: usize) -> usize {
		self.dsu.find(value)
	}

	fn node_for_root(&self, root: usize) -> usize {
		self.set_nodes[root]
	}

	fn size_for_root(&self, root: usize) -> usize {
		self.set_sizes[root]
	}

	fn merge_with_node(&mut self, lhs_root: usize, rhs_root: usize, node: usize) -> usize {
		let lhs_size = self.set_sizes[lhs_root];
		let rhs_size = self.set_sizes[rhs_root];
		let merged_root = self.dsu.union(lhs_root, rhs_root);
		self.set_sizes[merged_root] = lhs_size + rhs_size;
		self.set_nodes[merged_root] = node;
		merged_root
	}
}

pub fn cluster(data: Vec<Vec<f32>>, params: GraphHdbscanParams) -> anyhow::Result<Vec<i32>> {
	validate_params(&params)?;

	let point_count = data.len();
	if point_count == 0 {
		return Ok(Vec::new());
	}
	if point_count == 1 {
		return Ok(vec![-1]);
	}

	let target_k = (params.neighbors.max(params.min_samples)).min(point_count - 1);
	let (neighbors, core_distances) = build_approximate_knn(&data, target_k, &params);
	let mut edges = build_mutual_reachability_edges(&neighbors, &core_distances, params.epsilon);
	if edges.is_empty() {
		return Ok(vec![-1; point_count]);
	}

	edges.sort_by(|lhs, rhs| {
		lhs.u
			.cmp(&rhs.u)
			.then(lhs.v.cmp(&rhs.v))
			.then(lhs.weight.total_cmp(&rhs.weight))
	});

	let mut deduped_edges: Vec<WeightedEdge> = Vec::with_capacity(edges.len());
	for edge in edges {
		if let Some(last) = deduped_edges.last_mut()
			&& last.u == edge.u
			&& last.v == edge.v
		{
			if edge.weight < last.weight {
				last.weight = edge.weight;
			}
			continue;
		}

		deduped_edges.push(edge);
	}

	let components = build_components(point_count, &deduped_edges);
	let mut labels = vec![-1; point_count];
	let mut next_label = 0i32;

	for component in components {
		if component.points.len() < params.min_cluster_size {
			continue;
		}

		let clusters = select_component_clusters(&component, &params);
		for cluster in clusters {
			if cluster.len() < params.min_cluster_size {
				continue;
			}

			for local_point in cluster {
				let global_point = component.points[local_point];
				labels[global_point] = next_label;
			}
			next_label += 1;
		}
	}

	Ok(labels)
}

fn validate_params(params: &GraphHdbscanParams) -> anyhow::Result<()> {
	ensure!(
		params.min_cluster_size >= 2,
		"--min-cluster-size must be at least 2 for graph-hdbscan"
	);
	ensure!(
		params.min_samples >= 1,
		"--min-samples must be at least 1 for graph-hdbscan"
	);
	ensure!(
		params.neighbors >= 1,
		"--graph-neighbors must be at least 1"
	);
	ensure!(params.pivots >= 1, "--graph-pivots must be at least 1");
	ensure!(
		params.top_pivots >= 1,
		"--graph-top-pivots must be at least 1"
	);
	ensure!(
		params.top_pivots <= params.pivots,
		"--graph-top-pivots must be less than or equal to --graph-pivots"
	);
	ensure!(
		params.max_candidates >= params.neighbors,
		"--graph-max-candidates must be greater than or equal to --graph-neighbors"
	);
	ensure!(
		params.epsilon >= 0.0,
		"--epsilon must be greater than or equal to 0.0"
	);
	Ok(())
}

fn build_approximate_knn(
	data: &[Vec<f32>],
	target_k: usize,
	params: &GraphHdbscanParams,
) -> (Vec<Vec<(usize, f32)>>, Vec<f32>) {
	let point_count = data.len();
	let pivot_count = params.pivots.min(point_count);
	let pivot_indices = choose_pivots(point_count, pivot_count, params.random_seed);
	let mut pivot_buckets = vec![Vec::new(); pivot_indices.len()];
	let mut point_pivots: Vec<Vec<usize>> = Vec::with_capacity(point_count);

	for (point_index, point) in data.iter().enumerate() {
		let assigned = select_top_pivots(point, data, &pivot_indices, params.top_pivots);
		for &pivot_slot in &assigned {
			pivot_buckets[pivot_slot].push(point_index);
		}
		point_pivots.push(assigned);
	}

	let mut marks = vec![0u32; point_count];
	let mut epoch = 1u32;
	let mut knn = vec![Vec::new(); point_count];
	let mut core_distances = vec![0.0f32; point_count];

	for point_index in 0..point_count {
		epoch = epoch.wrapping_add(1);
		if epoch == 0 {
			epoch = 1;
			marks.fill(0);
		}

		let mut candidates = Vec::with_capacity(params.max_candidates);
		for &pivot_slot in &point_pivots[point_index] {
			for &candidate in &pivot_buckets[pivot_slot] {
				if candidate == point_index || marks[candidate] == epoch {
					continue;
				}
				marks[candidate] = epoch;
				candidates.push(candidate);
				if candidates.len() >= params.max_candidates {
					break;
				}
			}
			if candidates.len() >= params.max_candidates {
				break;
			}
		}

		if candidates.len() < target_k {
			for offset in 1..point_count {
				let candidate = (point_index + offset) % point_count;
				if candidate == point_index || marks[candidate] == epoch {
					continue;
				}
				marks[candidate] = epoch;
				candidates.push(candidate);
				if candidates.len() >= target_k {
					break;
				}
			}
		}

		let mut scored_candidates: Vec<(usize, f32)> = candidates
			.into_iter()
			.map(|candidate| {
				(
					candidate,
					cosine_distance(&data[point_index], &data[candidate]),
				)
			})
			.collect();
		scored_candidates.sort_by(|lhs, rhs| lhs.1.total_cmp(&rhs.1).then(lhs.0.cmp(&rhs.0)));

		let keep = target_k.min(scored_candidates.len());
		if keep == 0 {
			core_distances[point_index] = f32::INFINITY;
			continue;
		}

		let trimmed = scored_candidates.into_iter().take(keep).collect::<Vec<_>>();
		core_distances[point_index] = trimmed
			.last()
			.map(|(_, distance)| *distance)
			.unwrap_or(f32::INFINITY);
		knn[point_index] = trimmed;
	}

	(knn, core_distances)
}

fn choose_pivots(count: usize, pivot_count: usize, seed: u64) -> Vec<usize> {
	if pivot_count >= count {
		return (0..count).collect();
	}

	let mut selected = HashSet::with_capacity(pivot_count);
	let mut state = splitmix64(seed ^ 0xA5A5_A5A5_A5A5_A5A5);

	while selected.len() < pivot_count {
		state = splitmix64(state);
		selected.insert((state as usize) % count);
	}

	let mut pivots: Vec<usize> = selected.into_iter().collect();
	pivots.sort_unstable();
	pivots
}

fn select_top_pivots(
	point: &[f32],
	data: &[Vec<f32>],
	pivot_indices: &[usize],
	top_pivots: usize,
) -> Vec<usize> {
	let mut best: Vec<(f32, usize)> = Vec::with_capacity(top_pivots);

	for (pivot_slot, pivot_index) in pivot_indices.iter().copied().enumerate() {
		let distance = cosine_distance(point, &data[pivot_index]);
		if best.len() < top_pivots {
			best.push((distance, pivot_slot));
			best.sort_by(|lhs, rhs| lhs.0.total_cmp(&rhs.0).then(lhs.1.cmp(&rhs.1)));
			continue;
		}

		if let Some((worst_distance, _)) = best.last().copied()
			&& distance < worst_distance
		{
			let tail = best.len() - 1;
			best[tail] = (distance, pivot_slot);
			best.sort_by(|lhs, rhs| lhs.0.total_cmp(&rhs.0).then(lhs.1.cmp(&rhs.1)));
		}
	}

	if best.is_empty() {
		return vec![0];
	}

	best.into_iter().map(|(_, slot)| slot).collect()
}

fn build_mutual_reachability_edges(
	neighbors: &[Vec<(usize, f32)>],
	core_distances: &[f32],
	epsilon: f32,
) -> Vec<WeightedEdge> {
	let mut edges = Vec::new();

	for (point_index, point_neighbors) in neighbors.iter().enumerate() {
		for &(neighbor_index, raw_distance) in point_neighbors {
			let (u, v) = if point_index < neighbor_index {
				(point_index, neighbor_index)
			} else {
				(neighbor_index, point_index)
			};

			let weight = core_distances[point_index]
				.max(core_distances[neighbor_index])
				.max(raw_distance + epsilon);

			edges.push(WeightedEdge { u, v, weight });
		}
	}

	edges
}

fn build_components(point_count: usize, edges: &[WeightedEdge]) -> Vec<GraphComponent> {
	let mut dsu = DisjointSet::new(point_count);
	for edge in edges {
		dsu.union(edge.u, edge.v);
	}

	let roots = (0..point_count)
		.map(|point| dsu.find(point))
		.collect::<Vec<_>>();

	let mut points_by_root: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
	for (point, root) in roots.iter().copied().enumerate() {
		points_by_root.entry(root).or_default().push(point);
	}

	let mut edges_by_root: BTreeMap<usize, Vec<WeightedEdge>> = BTreeMap::new();
	for edge in edges {
		let root = roots[edge.u];
		edges_by_root.entry(root).or_default().push(*edge);
	}

	let mut components = Vec::with_capacity(points_by_root.len());
	for (root, points) in points_by_root {
		let edges = edges_by_root.remove(&root).unwrap_or_default();
		components.push(GraphComponent { points, edges });
	}

	components
}

fn select_component_clusters(
	component: &GraphComponent,
	params: &GraphHdbscanParams,
) -> Vec<Vec<usize>> {
	let point_count = component.points.len();
	if point_count < params.min_cluster_size {
		return Vec::new();
	}
	if component.edges.is_empty() {
		return Vec::new();
	}

	let mut local_index = BTreeMap::new();
	for (local, global) in component.points.iter().copied().enumerate() {
		local_index.insert(global, local);
	}

	let mut local_edges = Vec::with_capacity(component.edges.len());
	for edge in &component.edges {
		if let (Some(&u), Some(&v)) = (local_index.get(&edge.u), local_index.get(&edge.v)) {
			local_edges.push(WeightedEdge {
				u,
				v,
				weight: edge.weight,
			});
		}
	}
	if local_edges.is_empty() {
		return Vec::new();
	}

	local_edges.sort_by(|lhs, rhs| lhs.weight.total_cmp(&rhs.weight));
	let mut nodes = Vec::with_capacity(2 * point_count - 1);
	for _ in 0..point_count {
		nodes.push(DendrogramNode {
			left: None,
			right: None,
			size: 1,
			merge_distance: 0.0,
		});
	}

	let mut kruskal = KruskalState::new(point_count);
	let mut max_merge_distance = 0.0f32;
	let mut merge_count = 0usize;

	for edge in local_edges {
		let lhs_root = kruskal.find(edge.u);
		let rhs_root = kruskal.find(edge.v);
		if lhs_root == rhs_root {
			continue;
		}

		max_merge_distance = max_merge_distance.max(edge.weight);
		let lhs_node = kruskal.node_for_root(lhs_root);
		let rhs_node = kruskal.node_for_root(rhs_root);
		let merged_size = kruskal.size_for_root(lhs_root) + kruskal.size_for_root(rhs_root);

		let node_id = nodes.len();
		nodes.push(DendrogramNode {
			left: Some(lhs_node),
			right: Some(rhs_node),
			size: merged_size,
			merge_distance: edge.weight,
		});

		kruskal.merge_with_node(lhs_root, rhs_root, node_id);
		merge_count += 1;
	}

	if merge_count == 0 {
		return Vec::new();
	}

	let root = {
		let root_set = kruskal.find(0);
		kruskal.node_for_root(root_set)
	};
	let root_parent_distance = max_merge_distance + params.epsilon.max(1e-3);
	let pick_node = select_cluster_nodes(root, root_parent_distance, &nodes, params);

	let mut selected_nodes = Vec::new();
	collect_selected_nodes(root, &nodes, &pick_node, &mut selected_nodes);

	if selected_nodes.is_empty() {
		return Vec::new();
	}

	let mut clusters = Vec::new();
	for node in selected_nodes {
		let mut leaves = Vec::new();
		collect_leaf_points(node, &nodes, &mut leaves);
		leaves.sort_unstable();
		leaves.dedup();
		if leaves.len() >= params.min_cluster_size {
			clusters.push(leaves);
		}
	}

	clusters
}

fn select_cluster_nodes(
	root: usize,
	root_parent_distance: f32,
	nodes: &[DendrogramNode],
	params: &GraphHdbscanParams,
) -> Vec<bool> {
	let mut pick_node = vec![false; nodes.len()];
	let mut best_score = vec![0.0f64; nodes.len()];
	let mut parent_distance = vec![0.0f32; nodes.len()];
	let mut order = Vec::with_capacity(nodes.len());
	let mut stack = Vec::with_capacity(nodes.len());

	parent_distance[root] = root_parent_distance;
	stack.push(root);

	while let Some(node) = stack.pop() {
		order.push(node);
		if let (Some(left), Some(right)) = (nodes[node].left, nodes[node].right) {
			parent_distance[left] = nodes[node].merge_distance;
			parent_distance[right] = nodes[node].merge_distance;
			stack.push(left);
			stack.push(right);
		}
	}

	for node in order.into_iter().rev() {
		let current = nodes[node];
		if current.size < params.min_cluster_size {
			pick_node[node] = false;
			best_score[node] = 0.0;
			continue;
		}

		let child_score = match (current.left, current.right) {
			(Some(left), Some(right)) => best_score[left] + best_score[right],
			_ => 0.0,
		};

		let interval = (parent_distance[node] - current.merge_distance).max(0.0) as f64;
		let mut node_score = interval * current.size as f64;
		if node == root {
			node_score = f64::NEG_INFINITY;
		}

		if node_score >= child_score {
			pick_node[node] = true;
			best_score[node] = node_score.max(0.0);
		} else {
			pick_node[node] = false;
			best_score[node] = child_score;
		}
	}

	pick_node
}

fn collect_selected_nodes(
	node: usize,
	nodes: &[DendrogramNode],
	pick_node: &[bool],
	selected: &mut Vec<usize>,
) {
	let mut stack = vec![node];
	while let Some(current) = stack.pop() {
		if pick_node[current] {
			selected.push(current);
			continue;
		}

		if let (Some(left), Some(right)) = (nodes[current].left, nodes[current].right) {
			stack.push(right);
			stack.push(left);
		}
	}
}

fn collect_leaf_points(node: usize, nodes: &[DendrogramNode], leaves: &mut Vec<usize>) {
	let mut stack = vec![node];
	while let Some(current) = stack.pop() {
		if let (Some(left), Some(right)) = (nodes[current].left, nodes[current].right) {
			stack.push(right);
			stack.push(left);
			continue;
		}

		leaves.push(current);
	}
}

fn cosine_distance(lhs: &[f32], rhs: &[f32]) -> f32 {
	let dot = lhs
		.iter()
		.zip(rhs.iter())
		.map(|(left, right)| left * right)
		.sum::<f32>();
	if !dot.is_finite() {
		return 1.0;
	}
	(1.0 - dot).max(0.0)
}

fn splitmix64(mut x: u64) -> u64 {
	x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
	x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
	x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
	x ^ (x >> 31)
}

#[cfg(test)]
mod tests {
	use super::{GraphHdbscanParams, cluster};

	fn normalize(vector: [f32; 3]) -> Vec<f32> {
		let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
		vector.iter().map(|value| *value / norm).collect()
	}

	#[test]
	fn separates_two_compact_groups_and_noise() {
		let mut data = Vec::new();
		for x in [0.98f32, 1.0, 1.02, 0.97, 1.01] {
			data.push(normalize([x, 0.02, 0.01]));
		}
		for y in [0.99f32, 1.01, 1.03, 0.98, 1.0] {
			data.push(normalize([0.02, y, 0.01]));
		}
		data.push(normalize([0.01, 0.02, 1.0]));

		let params = GraphHdbscanParams {
			min_cluster_size: 3,
			min_samples: 2,
			epsilon: 0.02,
			neighbors: 4,
			pivots: 4,
			top_pivots: 2,
			max_candidates: 12,
			random_seed: 42,
		};

		let labels = cluster(data, params).expect("graph-hdbscan should cluster synthetic data");
		let noise_count = labels.iter().filter(|&&label| label == -1).count();
		let cluster_count = labels
			.iter()
			.filter(|&&label| label >= 0)
			.copied()
			.collect::<std::collections::BTreeSet<_>>()
			.len();

		assert!(
			cluster_count >= 2,
			"expected at least two non-noise clusters"
		);
		assert!(noise_count >= 1, "expected at least one noise point");
	}

	#[test]
	fn deterministic_for_same_seed() {
		let data = vec![
			normalize([1.0, 0.0, 0.0]),
			normalize([0.99, 0.01, 0.0]),
			normalize([0.0, 1.0, 0.0]),
			normalize([0.01, 0.99, 0.0]),
			normalize([0.0, 0.0, 1.0]),
		];
		let params = GraphHdbscanParams {
			min_cluster_size: 2,
			min_samples: 1,
			epsilon: 0.01,
			neighbors: 2,
			pivots: 3,
			top_pivots: 2,
			max_candidates: 8,
			random_seed: 9,
		};

		let labels_a = cluster(data.clone(), params).expect("first run should succeed");
		let labels_b = cluster(data, params).expect("second run should succeed");
		assert_eq!(labels_a, labels_b);
	}
}
