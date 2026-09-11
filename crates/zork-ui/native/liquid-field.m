#import <Foundation/Foundation.h>
#import <Metal/Metal.h>
#include <stdint.h>
#include <string.h>

typedef struct {
    float width,height,scale,plate_y;
    float plate_height,radius,member_fusion,dock_fusion;
    float surface[4],lower[4];
    uint32_t count,pixel_width,pixel_height;
    float surface_radius;
} ZorkLiquidParameters;

static id<MTLDevice> liquidDevice;
static id<MTLComputePipelineState> liquidPipeline;
static id<MTLCommandQueue> liquidQueue;

int zork_liquid_prepare(const uint8_t *bytes,size_t length) {
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        liquidDevice=MTLCreateSystemDefaultDevice();
        if(!liquidDevice) return;
        dispatch_data_t data=dispatch_data_create(bytes,length,dispatch_get_global_queue(QOS_CLASS_USER_INTERACTIVE,0),DISPATCH_DATA_DESTRUCTOR_DEFAULT);
        NSError *error=nil;
        id<MTLLibrary> library=[liquidDevice newLibraryWithData:data error:&error];
        id<MTLFunction> function=[library newFunctionWithName:@"liquid_field"];
        if(!function) return;
        liquidPipeline=[liquidDevice newComputePipelineStateWithFunction:function error:&error];
        liquidQueue=[liquidDevice newCommandQueue];
    });
    return liquidPipeline && liquidQueue;
}
@interface ZorkLiquidGPU : NSObject
@property(nonatomic,strong) id<MTLBuffer> pixels;
@property(nonatomic,strong) id<MTLBuffer> bubbles;
@end
@implementation ZorkLiquidGPU
@end
void *zork_liquid_create(void) {
    return liquidPipeline ? (__bridge_retained void*)[ZorkLiquidGPU new] : NULL;
}
const uint8_t *zork_liquid_render(void *handle,const ZorkLiquidParameters *parameters,
                                const float *bubbles,double *gpu_ms) {
    @autoreleasepool {
        ZorkLiquidGPU *state=(__bridge ZorkLiquidGPU*)handle;
        NSUInteger size=(NSUInteger)parameters->pixel_width*parameters->pixel_height*4;
        NSUInteger bubbleSize=MAX(16,(NSUInteger)parameters->count*16);
        if(!state.pixels || state.pixels.length<size)
            state.pixels=[liquidDevice newBufferWithLength:size options:MTLResourceStorageModeShared];
        if(!state.bubbles || state.bubbles.length<bubbleSize)
            state.bubbles=[liquidDevice newBufferWithLength:bubbleSize options:MTLResourceStorageModeShared];
        if(!state.pixels || !state.bubbles) return NULL;
        if(parameters->count) memcpy(state.bubbles.contents,bubbles,parameters->count*16);
        id<MTLCommandBuffer> command=[liquidQueue commandBuffer];
        command.label=@"Zork liquid field";
        id<MTLComputeCommandEncoder> encoder=[command computeCommandEncoder];
        [encoder setComputePipelineState:liquidPipeline];
        [encoder setBuffer:state.pixels offset:0 atIndex:0];
        [encoder setBytes:parameters length:sizeof(*parameters) atIndex:1];
        [encoder setBuffer:state.bubbles offset:0 atIndex:2];
        NSUInteger width=liquidPipeline.threadExecutionWidth;
        NSUInteger height=MIN(8,liquidPipeline.maxTotalThreadsPerThreadgroup/width);
        [encoder dispatchThreads:MTLSizeMake(parameters->pixel_width,parameters->pixel_height,1)
            threadsPerThreadgroup:MTLSizeMake(width,height,1)];
        [encoder endEncoding];
        [command commit];
        [command waitUntilCompleted];
        if(command.status==MTLCommandBufferStatusError) return NULL;
        *gpu_ms=MAX(0,(command.GPUEndTime-command.GPUStartTime)*1000.0);
        return state.pixels.contents;
    }
}
void zork_liquid_destroy(void *handle) { if(handle) { id owner=(__bridge_transfer id)handle; (void)owner; } }
