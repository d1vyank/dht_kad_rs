docker:
	@docker build -t dht_kad_rs .

run-testbed: docker
	@cd testbed && docker-compose up --scale=dht=10
